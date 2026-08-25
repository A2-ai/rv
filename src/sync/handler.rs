use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::consts::{BASE_PACKAGES, NO_CHECK_OPEN_FILE_ENV_VAR_NAME, RECOMMENDED_PACKAGES};
use crate::events;
use crate::lockfile::Source;
use crate::package::PackageType;
#[cfg(feature = "cli")]
use crate::r_cmd::kill_all_r_processes;
use crate::r_cmd::{RCmdError, RCmdErrorKind};
use crate::sync::changes::{CacheSource, SyncChange};
use crate::sync::errors::{SyncError, SyncErrorKind, SyncErrors};
use crate::sync::tasks::{install_task, sync_task};
use crate::sync::{LinkMode, sources};
use crate::utils::{get_max_workers, is_env_var_truthy};
use crate::{
    BuildPlan, BuildStep, Cancellation, Context, GitExecutor, RCmd, ResolvedDependency,
    get_tarball_urls,
};
use crossbeam::{channel, thread};
#[cfg(feature = "cli")]
use fs_err as fs;
use indicatif::{ProgressBar, ProgressStyle};
#[cfg(not(feature = "cli"))]
use std::fs;

fn get_all_packages_in_use(path: &Path) -> HashMap<(String, u32), HashSet<String>> {
    if !cfg!(unix) {
        return HashMap::new();
    }

    if is_env_var_truthy(NO_CHECK_OPEN_FILE_ENV_VAR_NAME) {
        return HashMap::new();
    }

    // lsof +D rv/ | awk 'NR>1 {print $2, $NF}' (to get PID and filename)
    let output = match std::process::Command::new("lsof")
        .arg("+D")
        .arg(path)
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::error!("lsof error: {e}. The +D option might not be available");
            return HashMap::new();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut out: HashMap<(String, u32), HashSet<String>> = HashMap::new();
    for (i, line) in stdout.lines().enumerate() {
        // Skip header
        if i == 0 {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 3 {
            // Process name is the first field (index 0), PID is the second field (index 1), filename is the last field
            if let (Ok(pid), Some(filename)) = (fields[1].parse::<u32>(), fields.last()) {
                let process_name = fields[0].to_string();
                // that should be a .so file in libs subfolder so we need to find grandparent
                let p = Path::new(filename);
                if let Some(parent) = p.parent().and_then(|p| p.parent())
                    && let Some(package_name) = parent.file_name().and_then(|n| n.to_str())
                {
                    out.entry((process_name, pid))
                        .or_default()
                        .insert(package_name.to_string());
                }
            }
        }
    }

    log::debug!("Packages with files loaded (via lsof): {out:?}");

    out
}

fn remove_package_path(p: &Path) -> std::io::Result<()> {
    if fs::symlink_metadata(p)?.file_type().is_symlink() {
        fs::remove_file(p)
    } else {
        fs::remove_dir_all(p)
    }
}

fn move_package_into_library(staged: &Path, dest: &Path, backup: &Path) -> std::io::Result<()> {
    // Use symlink_metadata so an existing symlink dest is detected too:
    // `Path::is_dir` follows symlinks and would miss a broken one.
    if fs::symlink_metadata(dest).is_ok() {
        if fs::symlink_metadata(backup).is_ok() {
            remove_package_path(backup)?;
        }
        fs::rename(dest, backup)?;
        fs::rename(staged, dest)?;
        remove_package_path(backup)?;
    } else {
        fs::rename(staged, dest)?;
    }
    Ok(())
}

#[derive(Debug)]
pub struct SyncHandler<'a> {
    context: &'a Context,
    save_install_logs_in: Option<PathBuf>,
    dry_run: bool,
    show_progress_bar: bool,
    max_workers: usize,
    uses_lockfile: bool,
}

impl<'a> SyncHandler<'a> {
    pub fn new(context: &'a Context, save_install_logs_in: Option<PathBuf>) -> Self {
        Self {
            context,
            save_install_logs_in,
            dry_run: false,
            show_progress_bar: false,
            uses_lockfile: false,
            max_workers: get_max_workers(),
        }
    }

    pub fn dry_run(&mut self) {
        self.dry_run = true;
    }

    pub fn show_progress_bar(&mut self) {
        self.show_progress_bar = true;
    }

    /// Assigning a value <= 0 is a no-op
    pub fn set_max_workers(&mut self, max_workers: usize) {
        if max_workers > 0 {
            self.max_workers = max_workers;
        }
    }

    pub fn set_uses_lockfile(&mut self, uses_lockfile: bool) {
        self.uses_lockfile = uses_lockfile;
    }

    /// Download source tarballs for all Repository dependencies without installing.
    /// Useful for archival/backup purposes.
    /// Returns paths to downloaded tarballs.
    pub fn download_tarballs(
        &self,
        deps: &[ResolvedDependency],
    ) -> Result<Vec<PathBuf>, SyncError> {
        let repo_deps: Vec<_> = deps
            .iter()
            .filter(|d| matches!(&d.source, Source::Repository { .. }))
            .collect();

        if repo_deps.is_empty() {
            return Ok(Vec::new());
        }

        let pb = if self.show_progress_bar {
            let pb = ProgressBar::new(repo_deps.len() as u64);
            pb.set_style(
                ProgressStyle::with_template("[{elapsed_precise}] {bar:60} {pos:>7}/{len:7} {msg}")
                    .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_secs(1));
            Arc::new(pb)
        } else {
            Arc::new(ProgressBar::hidden())
        };

        let (work_sender, work_receiver) = channel::unbounded();
        let (done_sender, done_receiver) =
            channel::unbounded::<Result<(String, PathBuf), (String, crate::http::HttpError)>>();

        // Queue all work
        for dep in &repo_deps {
            work_sender
                .send(*dep)
                .expect("failed to enqueue download work item: work_receiver dropped unexpectedly");
        }
        drop(work_sender);

        let downloaded = Arc::new(Mutex::new(Vec::new()));
        let errors: Arc<Mutex<Vec<(String, crate::http::HttpError)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let downloading = Arc::new(Mutex::new(HashSet::new()));

        thread::scope(|s| {
            // Spawn max_workers threads
            for _ in 0..self.max_workers {
                let work_receiver = work_receiver.clone();
                let done_sender = done_sender.clone();
                let pb = Arc::clone(&pb);
                let downloading = Arc::clone(&downloading);

                s.spawn(move |_| {
                    while let Ok(dep) = work_receiver.recv() {
                        let name = dep.name.to_string();
                        {
                            let mut d = downloading.lock().unwrap();
                            d.insert(name.clone());
                            pb.set_message(format!("Downloading {d:?}"));
                        }

                        // safe unwrap, we know it's a repo dep
                        let tarball_url = get_tarball_urls(
                            dep,
                            self.context.cache.r_version(),
                            self.context.cache.system_info(),
                        )
                        .unwrap();

                        let tarball_path = self
                            .context
                            .cache
                            .local()
                            .get_tarball_path(&dep.name, &dep.version.original);

                        let result = crate::http::download_to_file(
                            &tarball_url.source,
                            &tarball_path,
                        )
                        .or_else(|e| {
                            log::warn!(
                                "Failed to download source tarball from {}: {e}, trying archive",
                                tarball_url.source
                            );
                            crate::http::download_to_file(
                                &tarball_url.source_archive,
                                &tarball_path,
                            )
                        });

                        // Send result with name for tracking
                        match result {
                            Ok(_) => done_sender.send(Ok((name, tarball_path))).expect(
                                "done_receiver dropped while sending successful download result",
                            ),
                            Err(e) => done_sender.send(Err((name, e))).expect(
                                "done_receiver dropped while sending failed download result",
                            ),
                        }
                    }
                });
            }
            drop(done_sender);

            // Collect results - continue on errors
            for result in done_receiver {
                let name = match result {
                    Ok((name, path)) => {
                        downloaded.lock().unwrap().push(path);
                        name
                    }
                    Err((name, e)) => {
                        errors.lock().unwrap().push((name.clone(), e));
                        name
                    }
                };
                let mut d = downloading.lock().unwrap();
                d.remove(&name);
                pb.inc(1);
                pb.set_message(format!("Downloading {d:?}"));
            }
        })
        .expect("threads to not panic");

        pb.finish_and_clear();

        let errors = Arc::try_unwrap(errors).unwrap().into_inner().unwrap();
        if !errors.is_empty() {
            // Log all errors but still return successful downloads
            for (name, e) in &errors {
                log::error!("Failed to download {name}: {e}");
            }
        }

        Ok(Arc::try_unwrap(downloaded).unwrap().into_inner().unwrap())
    }

    /// Resolve configure_args for a package based on current system info
    fn get_configure_args(&self, package_name: &str) -> Vec<String> {
        if let Some(rules) = self.context.config.configure_args().get(package_name) {
            // Find first matching rule
            for rule in rules {
                if let Some(args) = rule.matches(self.context.cache.system_info()) {
                    return args.to_vec();
                }
            }
        }

        Vec::new()
    }

    /// Check whether stripping should be applied for a package.
    /// Returns false if the package is listed in [project.no_strip].
    fn should_strip(&self, package_name: &str) -> bool {
        !self
            .context
            .config
            .no_strip()
            .iter()
            .any(|name| name == package_name)
    }

    fn copy_package(&self, dep: &ResolvedDependency) -> Result<(), SyncError> {
        if self.dry_run {
            return Ok(());
        }

        log::debug!("Copying package {} from current library", dep.name);
        LinkMode::link_files(
            Some(LinkMode::Copy),
            &dep.name,
            self.context.library.path().join(dep.name.as_ref()),
            self.context.staging_path().join(dep.name.as_ref()),
        )?;

        Ok(())
    }

    fn install_package(
        &self,
        dep: &ResolvedDependency,
        r_cmd: &impl RCmd,
        cancellation: Arc<Cancellation>,
    ) -> Result<(), SyncError> {
        if self.dry_run {
            return Ok(());
        }
        // we want the staging to take precedence over the library, but still have
        // the library in the paths for lookup
        let staging_path = self.context.staging_path();
        let library_dirs = vec![&staging_path, self.context.library.path()];
        let configure_args = self.get_configure_args(&dep.name);
        let strip = self.should_strip(&dep.name);

        match dep.source {
            Source::Repository { .. } => sources::repositories::install_package(
                dep,
                &library_dirs,
                &self.context.cache,
                r_cmd,
                &configure_args,
                strip,
                cancellation,
            ),
            Source::Git { .. } | Source::RUniverse { .. } => sources::git::install_package(
                dep,
                &library_dirs,
                &self.context.cache,
                r_cmd,
                &GitExecutor {},
                &configure_args,
                strip,
                cancellation,
            ),
            Source::Local { .. } => sources::local::install_package(
                dep,
                &self.context.project_dir,
                &library_dirs,
                self.context.cache.local(),
                r_cmd,
                &configure_args,
                strip,
                cancellation,
            ),
            Source::Url { .. } => sources::url::install_package(
                dep,
                &library_dirs,
                &self.context.cache,
                r_cmd,
                &configure_args,
                strip,
                cancellation,
            ),
            Source::Builtin { .. } => Ok(()),
        }
    }

    /// We want to figure out:
    /// 1. if there are packages in there not the list of deps (eg to remove)
    /// 2. if all the packages are already installed at the right version
    /// 3. if there are some local packages we can copy
    ///
    /// If we don't have a lockfile, we just skip the whole thing and pretend we don't have a library
    fn compare_with_local_library(
        &self,
        deps: &[ResolvedDependency],
    ) -> (HashSet<&str>, HashSet<&str>, HashSet<(&str, bool)>) {
        let mut deps_seen = HashSet::new();
        let mut deps_to_copy = HashSet::new();
        // (name, notify). We do not notify if the package is broken in some ways.
        let mut deps_to_remove = HashSet::new();

        let deps_by_name: HashMap<_, _> = deps.iter().map(|d| (d.name.as_ref(), d)).collect();

        for name in self.context.library.packages.keys() {
            if let Some(dep) = deps_by_name.get(name.as_str()) {
                // If the library contains the dep, we also want it to be resolved from the lockfile, otherwise we cannot trust its source
                // Additionally, any package in the library that is ignored, needs to be removed
                if self.context.library.contains_package(dep) && !dep.ignored {
                    match &dep.source {
                        Source::Repository { .. } => {
                            if !self.uses_lockfile || dep.from_lockfile {
                                deps_seen.insert(name.as_str());
                            }
                        }
                        Source::Git { .. } | Source::RUniverse { .. } | Source::Url { .. } => {
                            deps_seen.insert(name.as_str());
                        }
                        Source::Local { .. } => {
                            deps_to_copy.insert(name.as_str());
                            deps_seen.insert(name.as_str());
                        }
                        _ => (),
                    }
                    continue;
                }
            }
            deps_to_remove.insert((name.as_str(), true));
        }

        // Skip builtin versions
        let mut out = Vec::from(RECOMMENDED_PACKAGES);
        out.extend(BASE_PACKAGES.as_slice());
        for name in out {
            if let Some(dep) = deps_by_name.get(name)
                && dep.source.is_builtin()
            {
                deps_seen.insert(name);
            }
        }

        // Lastly, remove any package that we can't really access
        for name in &self.context.library.broken {
            log::warn!("Package {name} in library is broken");
            deps_to_remove.insert((name.as_str(), false));
        }

        (deps_seen, deps_to_copy, deps_to_remove)
    }

    pub fn handle(
        &self,
        deps: &[ResolvedDependency],
        r_cmd: &impl RCmd,
    ) -> Result<Vec<SyncChange>, SyncError> {
        events::with_task(sync_task(), || self.handle_impl(deps, r_cmd))
    }

    fn handle_impl(
        &self,
        deps: &[ResolvedDependency],
        r_cmd: &impl RCmd,
    ) -> Result<Vec<SyncChange>, SyncError> {
        // Clean up at all times, even with a dry run
        let cancellation = Arc::new(Cancellation::default());

        let staging_path = self.context.staging_path();
        #[cfg(feature = "cli")]
        {
            let cancellation_clone = Arc::clone(&cancellation);
            let staging_path = staging_path.clone();
            ctrlc::set_handler(move || {
                cancellation_clone.cancel();
                if cancellation_clone.is_soft_cancellation() {
                    println!(
                        "Finishing current operations... Press Ctrl+C again to exit immediately."
                    );
                } else if cancellation_clone.is_hard_cancellation() {
                    kill_all_r_processes();
                    if staging_path.is_dir() {
                        fs::remove_dir_all(&staging_path).expect("Failed to remove staging path");
                    }
                    ::std::process::exit(130);
                }
            })
            .expect("Error setting Ctrl-C handler");
        }

        if cancellation.is_cancelled() {
            return Ok(Vec::new());
        }

        if staging_path.is_dir() {
            fs::remove_dir_all(&staging_path)?;
        }
        fs::create_dir_all(self.context.library.path())?;

        let mut sync_changes = Vec::new();

        let mut plan = BuildPlan::new(deps);
        let num_deps_to_install = plan.num_to_install();
        let (deps_seen, deps_to_copy, deps_to_remove) = self.compare_with_local_library(deps);
        let needs_sync = deps_seen.len() != num_deps_to_install;
        let packages_loaded = if !deps_to_remove.is_empty() {
            get_all_packages_in_use(self.context.library.path())
        } else {
            HashMap::new()
        };

        for (dir_name, notify) in &deps_to_remove {
            if packages_loaded
                .values()
                .any(|packages| packages.contains(*dir_name))
            {
                log::debug!(
                    "{dir_name} in the library is loaded in a session but we want to remove it."
                );
                return Err(SyncError {
                    source: SyncErrorKind::PackagesLoadedError(
                        packages_loaded
                            .iter()
                            .map(|((process_name, pid), packages)| {
                                format!(
                                    "{} ({}): {}",
                                    process_name,
                                    pid,
                                    packages
                                        .iter()
                                        .map(|s| s.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                });
            }

            if *notify {
                sync_changes.push(SyncChange::removed(dir_name));
            }

            // Only actually remove the deps if we are not going to do any other changes.
            if !needs_sync {
                let p = self.context.library.path().join(dir_name);
                if !self.dry_run && *notify {
                    log::debug!("Removing {dir_name} from library");
                    fs::remove_dir_all(&p)?;
                }
            }
        }

        // If we have all the deps we need, exit early
        if !needs_sync {
            return Ok(sync_changes);
        }

        // Create staging only if we need to build stuff
        fs::create_dir_all(&staging_path)?;

        if let Some(log_folder) = &self.save_install_logs_in {
            fs::create_dir_all(log_folder)?;
        }

        // Then we mark the deps seen so they won't be installed into the staging dir
        for d in &deps_seen {
            // builtin packages will not be in the library
            let in_lib = self.context.library.path().join(d);
            if in_lib.is_dir() {
                plan.mark_installed(d);
            }
        }
        let num_deps_to_install = plan.num_to_install();

        // We can't use references from the BuildPlan since we borrow mutably from it so we
        // create a lookup table for resolved deps by name and use those references across channels.
        let dep_by_name: HashMap<_, _> = deps.iter().map(|d| (&d.name, d)).collect();

        let pb = if self.show_progress_bar {
            let pb = ProgressBar::new(plan.num_to_install() as u64);
            pb.set_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}] {bar:60} {pos:>7}/{len:7}\n{msg}",
                )
                .unwrap(),
            );
            pb.enable_steady_tick(Duration::from_secs(1));
            Arc::new(pb)
        } else {
            Arc::new(ProgressBar::hidden())
        };

        let (ready_sender, ready_receiver) = channel::unbounded();
        let (done_sender, done_receiver) = channel::unbounded();

        let plan = Arc::new(Mutex::new(plan));
        // Initial deps we can install immediately
        {
            let mut plan = plan.lock().unwrap();
            while let BuildStep::Install(d) = plan.get() {
                ready_sender.send(dep_by_name[&d.name]).unwrap();
            }
        }

        let installed_count = Arc::new(AtomicUsize::new(0));
        let has_errors = Arc::new(AtomicBool::new(false));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let deps_to_copy = Arc::new(deps_to_copy);

        thread::scope(|s| {
            let plan_clone = Arc::clone(&plan);
            let ready_sender_clone = ready_sender.clone();
            let installed_count_clone = Arc::clone(&installed_count);
            let has_errors_clone = Arc::clone(&has_errors);

            // Different thread to monitor what needs to be installed next
            s.spawn(move |_| {
                let mut seen = HashSet::new();
                while !has_errors_clone.load(Ordering::Relaxed)
                    && installed_count_clone.load(Ordering::Relaxed) < num_deps_to_install
                {
                    let mut plan = plan_clone.lock().unwrap();
                    let mut ready = Vec::new();
                    while let BuildStep::Install(d) = plan.get() {
                        ready.push(dep_by_name[&d.name]);
                    }
                    drop(plan); // Release lock before sending

                    for p in ready {
                        if !seen.contains(&p.name) {
                            seen.insert(&p.name);
                            ready_sender_clone.send(p).unwrap();
                        }
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                drop(ready_sender_clone);
            });
            let installing = Arc::new(Mutex::new(HashSet::new()));

            // Our worker threads that will actually perform the installation
            for worker_num in 0..self.max_workers {
                let ready_receiver = ready_receiver.clone();
                let done_sender = done_sender.clone();
                let plan = Arc::clone(&plan);
                let has_errors_clone = Arc::clone(&has_errors);
                let errors_clone = Arc::clone(&errors);
                let deps_to_copy_clone = Arc::clone(&deps_to_copy);
                let pb_clone = Arc::clone(&pb);
                let installing_clone = Arc::clone(&installing);
                let cancellation_clone = cancellation.clone();
                let save_install_logs_in_clone = self.save_install_logs_in.clone();

                s.spawn(move |_| {
                    let local_worker_id = worker_num + 1;
                    while let Ok(dep) = ready_receiver.recv() {
                        if has_errors_clone.load(Ordering::Relaxed)
                            || cancellation_clone.is_cancelled()
                        {
                            break;
                        }

                        installing_clone.lock().unwrap().insert(dep.name.clone());
                        let start = std::time::Instant::now();
                        events::emit(&events::Event::TaskStarted {
                            task: install_task(&dep.name),
                        });
                        if !self.dry_run {
                            if self.show_progress_bar {
                                pb_clone.set_message(format!(
                                    "Installing {:?}",
                                    installing_clone.lock().unwrap()
                                ));
                            }
                            match dep.kind {
                                PackageType::Source => {
                                    log::debug!(
                                        "Installing {} (source) on worker {}",
                                        dep.name,
                                        local_worker_id
                                    )
                                }
                                PackageType::Binary => {
                                    log::debug!(
                                        "Installing {} (binary) on worker {}",
                                        dep.name,
                                        local_worker_id
                                    )
                                }
                            }
                        }
                        let install_result = if deps_to_copy_clone.contains(dep.name.as_ref()) {
                            self.copy_package(dep)
                        } else {
                            self.install_package(dep, r_cmd, cancellation_clone.clone())
                        };

                        match install_result {
                            Ok(_) => {
                                let is_binary = dep.kind == PackageType::Binary;
                                let binary_cached =
                                    !is_binary && dep.cache_status.binary_available();
                                let cache_source = {
                                    if is_binary || binary_cached {
                                        if dep.cache_status.global_binary_available() {
                                            Some(CacheSource::Global)
                                        } else if dep.cache_status.local_binary_available() {
                                            Some(CacheSource::Local)
                                        } else {
                                            None // Downloaded
                                        }
                                    } else {
                                        // Source package without cached binary
                                        if dep
                                            .cache_status
                                            .global
                                            .map(|x| x.source_available())
                                            .unwrap_or(false)
                                        {
                                            Some(CacheSource::Global)
                                        } else if dep.cache_status.local.source_available() {
                                            Some(CacheSource::Local)
                                        } else {
                                            None // Downloaded
                                        }
                                    }
                                };
                                let sync_change = SyncChange::installed(
                                    &dep.name,
                                    &dep.version.original,
                                    dep.source.clone(),
                                    dep.kind,
                                    start.elapsed(),
                                    self.context
                                        .system_dependencies
                                        .get(dep.name.as_ref())
                                        .cloned()
                                        .unwrap_or_default(),
                                    cache_source,
                                    binary_cached,
                                );
                                let mut plan = plan.lock().unwrap();
                                plan.mark_installed(&dep.name);
                                drop(plan);
                                if let Some(log_folder) = &save_install_logs_in_clone
                                    && !sync_change.is_builtin()
                                {
                                    let log_path = sync_change.log_path(self.context.cache.local());
                                    if log_path.exists() {
                                        fs::copy(
                                            log_path,
                                            log_folder.join(format!("{}.log", sync_change.name)),
                                        )
                                        .expect("no error");
                                    }
                                }
                                events::emit(&events::Event::TaskFinished {
                                    task: install_task(&dep.name),
                                    result: events::TaskResult::Ok,
                                    time_ms: start.elapsed().as_millis() as u64,
                                });
                                if done_sender.send(sync_change).is_err() {
                                    break; // Channel closed
                                }
                            }
                            Err(e) => {
                                events::emit(&events::Event::TaskFinished {
                                    task: install_task(&dep.name),
                                    result: events::TaskResult::Failed,
                                    time_ms: start.elapsed().as_millis() as u64,
                                });
                                has_errors_clone.store(true, Ordering::Relaxed);

                                if let SyncErrorKind::RCmdError(RCmdError {
                                    source:
                                        RCmdErrorKind::InstallationFailed(msg)
                                        | RCmdErrorKind::BuildFailed(msg),
                                    ..
                                }) = &e.source
                                    && let Some(log_folder) = &save_install_logs_in_clone
                                {
                                    fs::write(
                                        log_folder.join(format!("{}.log", dep.name)),
                                        msg.as_bytes(),
                                    )
                                    .expect("to write files");
                                }

                                errors_clone.lock().unwrap().push((dep, e));
                                break;
                            }
                        }
                    }
                    drop(done_sender);
                });
            }

            // Monitor progress in the main thread
            loop {
                if has_errors.load(Ordering::Relaxed) {
                    break;
                }
                // timeout is necessary to avoid deadlock
                if let Ok(change) = done_receiver.recv_timeout(Duration::from_millis(1)) {
                    installed_count.fetch_add(1, Ordering::Relaxed);
                    installing.lock().unwrap().remove(change.name.as_str());
                    if !self.dry_run {
                        log::debug!(
                            "Completed installing {} ({}/{})",
                            change.name,
                            installed_count.load(Ordering::Relaxed),
                            num_deps_to_install
                        );
                        if self.show_progress_bar {
                            pb.inc(1);
                            pb.set_message(format!("Installing {:?}", installing.lock().unwrap()));
                        }
                    }
                    if !deps_seen.contains(change.name.as_str()) {
                        sync_changes.push(change);
                    }
                    if installed_count.load(Ordering::Relaxed) == num_deps_to_install
                        || has_errors.load(Ordering::Relaxed)
                    {
                        break;
                    }
                }
            }

            // Clean up
            drop(ready_sender);
        })
        .expect("threads to not panic");

        pb.finish_and_clear();

        if has_errors.load(Ordering::Relaxed) {
            let mut err = errors.lock().unwrap();
            let errors = std::mem::take(&mut *err)
                .into_iter()
                .map(|(d, e)| (d.name.to_string(), e))
                .collect();
            return Err(SyncError {
                source: SyncErrorKind::SyncFailed(SyncErrors { errors }),
            });
        }

        if self.dry_run {
            fs::remove_dir_all(&staging_path)?;
        } else {
            // If we are there, it means we are successful.

            // Delete the packages that need to be removed from the library
            for (name, notify) in deps_to_remove {
                if notify && !staging_path.join(name).is_dir() {
                    let p = self.context.library.path().join(name);
                    log::debug!("Removing {name} from library");
                    fs::remove_dir_all(&p)?;
                }
            }

            let mut staged = Vec::new();
            for entry in fs::read_dir(&staging_path)? {
                let entry = entry?;
                let ft = entry.file_type()?;
                if !ft.is_dir() && !ft.is_symlink() {
                    continue;
                }
                let path = entry.path();
                // not a valid utf-8 name, can be skipped since all packages names should be ascii
                let Some(name) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
                else {
                    continue;
                };
                if !deps_seen.contains(name.as_str()) {
                    staged.push((path, name));
                }
            }

            for (path, name) in staged {
                let out = self.context.library.path().join(&name);
                let backup = staging_path.join(format!(".rvbak-{name}"));
                move_package_into_library(&path, &out, &backup)?;
            }

            // Then delete staging
            fs::remove_dir_all(&staging_path)?;
        }

        // Sort all changes by a-z and fall back on installed status for things with the same name
        sync_changes.sort_unstable_by(|a, b| {
            match a.name.to_lowercase().cmp(&b.name.to_lowercase()) {
                std::cmp::Ordering::Equal => a.installed.cmp(&b.installed),
                ordering => ordering,
            }
        });

        Ok(sync_changes)
    }
}

#[cfg(test)]
mod tests {
    use super::move_package_into_library;
    use std::fs;
    use std::path::Path;

    fn write_pkg(dir: &Path, marker: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("DESCRIPTION"), marker).unwrap();
    }

    #[cfg(unix)]
    fn symlink_pkg(link: &Path, target: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    #[test]
    fn moves_staged_package_into_empty_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join("staged");
        let dest = tmp.path().join("dest");
        let backup = tmp.path().join("backup");
        write_pkg(&staged, "new");

        move_package_into_library(&staged, &dest, &backup).unwrap();

        assert_eq!(fs::read_to_string(dest.join("DESCRIPTION")).unwrap(), "new");
        assert!(!staged.exists());
    }

    #[test]
    fn replaces_existing_package_and_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join("staged");
        let dest = tmp.path().join("dest");
        let backup = tmp.path().join("backup");
        write_pkg(&staged, "new");
        write_pkg(&dest, "old");
        write_pkg(&backup, "stale");

        move_package_into_library(&staged, &dest, &backup).unwrap();

        assert_eq!(fs::read_to_string(dest.join("DESCRIPTION")).unwrap(), "new");
        assert!(!staged.exists());
        assert!(!backup.exists());
    }

    #[cfg(unix)]
    #[test]
    fn replaces_existing_symlink_and_cleans_up() {
        let tmp = tempfile::tempdir().unwrap();
        let old_cache = tmp.path().join("old_cache");
        let new_cache = tmp.path().join("new_cache");
        let staged = tmp.path().join("staged");
        let dest = tmp.path().join("dest");
        let backup = tmp.path().join("backup");
        write_pkg(&old_cache, "old");
        write_pkg(&new_cache, "new");
        symlink_pkg(&dest, &old_cache);
        symlink_pkg(&staged, &new_cache);

        move_package_into_library(&staged, &dest, &backup).unwrap();

        assert_eq!(fs::read_to_string(dest.join("DESCRIPTION")).unwrap(), "new");
        assert!(!staged.exists());
        assert!(fs::symlink_metadata(&backup).is_err());
        // Removing the backup must not have followed the symlink into the old cache.
        assert_eq!(
            fs::read_to_string(old_cache.join("DESCRIPTION")).unwrap(),
            "old"
        );
    }
}
