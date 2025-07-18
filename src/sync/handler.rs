use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crossbeam::{channel, thread};
#[cfg(feature = "cli")]
use ctrlc;
use fs_err as fs;
use indicatif::{ProgressBar, ProgressStyle};

use crate::config::ConfigureArgsRule;
use crate::consts::{BASE_PACKAGES, NO_CHECK_OPEN_FILE_ENV_VAR_NAME, RECOMMENDED_PACKAGES};
use crate::lockfile::Source;
use crate::package::PackageType;
#[cfg(feature = "cli")]
use crate::r_cmd::kill_all_r_processes;
use crate::r_cmd::{InstallError, InstallErrorKind};
use crate::sync::changes::SyncChange;
use crate::sync::errors::{SyncError, SyncErrorKind, SyncErrors};
use crate::sync::{LinkMode, sources};
use crate::utils::get_max_workers;
use crate::{
    BuildPlan, BuildStep, Cancellation, DiskCache, GitExecutor, Library, RCmd, ResolvedDependency,
    SystemInfo,
};

fn get_all_packages_in_use(path: &Path) -> HashSet<String> {
    if !cfg!(unix) {
        return HashSet::new();
    }
    let val = std::env::var(NO_CHECK_OPEN_FILE_ENV_VAR_NAME)
        .unwrap_or_default()
        .to_lowercase();

    if val == "true" || val == "1" {
        return HashSet::new();
    }

    // lsof +D rv/ | awk 'NR>1 {print $NF}'
    let output = match std::process::Command::new("lsof")
        .arg("+D")
        .arg(path)
        .output()
    {
        Ok(output) => output,
        Err(e) => {
            log::error!("lsof error: {e}. The +D option might not be available");
            return HashSet::new();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut out = HashSet::new();
    for (i, line) in stdout.lines().enumerate() {
        // Skip header
        if i == 0 {
            continue;
        }

        if let Some(filename) = line.split_whitespace().last() {
            // that should be a .so file in libs subfolder so we need to find grandparent
            let p = Path::new(filename);
            let lib = p.parent().unwrap().parent().unwrap();
            out.insert(lib.file_name().unwrap().to_str().unwrap().to_string());
        }
    }

    log::debug!("Packages with files loaded (via lsof): {out:?}");

    out
}

#[derive(Debug)]
pub struct SyncHandler<'a> {
    project_dir: &'a Path,
    library: &'a Library,
    cache: &'a DiskCache,
    system_dependencies: &'a HashMap<String, Vec<String>>,
    configure_args: &'a HashMap<String, Vec<ConfigureArgsRule>>,
    system_info: &'a SystemInfo,
    staging_path: PathBuf,
    save_install_logs_in: Option<PathBuf>,
    dry_run: bool,
    show_progress_bar: bool,
    max_workers: usize,
    uses_lockfile: bool,
}

impl<'a> SyncHandler<'a> {
    pub fn new(
        project_dir: &'a Path,
        library: &'a Library,
        cache: &'a DiskCache,
        system_dependencies: &'a HashMap<String, Vec<String>>,
        configure_args: &'a HashMap<String, Vec<ConfigureArgsRule>>,
        system_info: &'a SystemInfo,
        save_install_logs_in: Option<PathBuf>,
        staging_path: impl AsRef<Path>,
    ) -> Self {
        Self {
            project_dir,
            library,
            cache,
            system_dependencies,
            configure_args,
            system_info,
            save_install_logs_in,
            staging_path: staging_path.as_ref().to_path_buf(),
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

    pub fn set_max_workers(&mut self, max_workers: usize) {
        assert!(self.max_workers > 0);
        self.max_workers = max_workers;
    }

    pub fn set_uses_lockfile(&mut self, uses_lockfile: bool) {
        self.uses_lockfile = uses_lockfile;
    }

    /// Resolve configure_args for a package based on current system info
    fn get_configure_args(&self, package_name: &str) -> Vec<String> {
        if let Some(rules) = self.configure_args.get(package_name) {
            // Find first matching rule
            for rule in rules {
                if let Some(args) = rule.matches(self.system_info) {
                    return args.to_vec();
                }
            }
        }

        Vec::new()
    }

    fn copy_package(&self, dep: &ResolvedDependency) -> Result<(), SyncError> {
        if self.dry_run {
            return Ok(());
        }

        log::debug!("Copying package {} from current library", &dep.name);
        LinkMode::Copy.link_files(
            &dep.name,
            self.library.path().join(dep.name.as_ref()),
            self.staging_path.join(dep.name.as_ref()),
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
        let library_dirs = vec![&self.staging_path, self.library.path()];
        let configure_args = self.get_configure_args(&dep.name);

        match dep.source {
            Source::Repository { .. } => sources::repositories::install_package(
                dep,
                &library_dirs,
                self.cache,
                r_cmd,
                &configure_args,
                cancellation,
            ),
            Source::Git { .. } | Source::RUniverse { .. } => sources::git::install_package(
                dep,
                &library_dirs,
                self.cache,
                r_cmd,
                &GitExecutor {},
                &configure_args,
                cancellation,
            ),
            Source::Local { .. } => sources::local::install_package(
                dep,
                self.project_dir,
                &library_dirs,
                self.cache,
                r_cmd,
                &configure_args,
                cancellation,
            ),
            Source::Url { .. } => sources::url::install_package(
                dep,
                &library_dirs,
                self.cache,
                r_cmd,
                &configure_args,
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

        for name in self.library.packages.keys() {
            if let Some(dep) = deps_by_name.get(name.as_str()) {
                // If the library contains the dep, we also want it to be resolved from the lockfile, otherwise we cannot trust its source
                // Additionally, any package in the library that is ignored, needs to be removed
                if self.library.contains_package(dep) && !dep.ignored {
                    match &dep.source {
                        Source::Repository { .. } => {
                            if !self.uses_lockfile {
                                deps_seen.insert(name.as_str());
                            } else {
                                if dep.from_lockfile {
                                    deps_seen.insert(name.as_str());
                                }
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
            if let Some(dep) = deps_by_name.get(name) {
                if dep.source.is_builtin() {
                    deps_seen.insert(name);
                }
            }
        }

        // Lastly, remove any package that we can't really access
        for name in &self.library.broken {
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
        // Clean up at all times, even with a dry run
        let cancellation = Arc::new(Cancellation::default());

        #[cfg(feature = "cli")]
        {
            let cancellation_clone = Arc::clone(&cancellation);
            let staging_path = self.staging_path.clone();
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

        if self.staging_path.is_dir() {
            fs::remove_dir_all(&self.staging_path)?;
        }
        fs::create_dir_all(self.library.path())?;

        let mut sync_changes = Vec::new();

        let mut plan = BuildPlan::new(deps);
        let num_deps_to_install = plan.num_to_install();
        let (deps_seen, deps_to_copy, deps_to_remove) = self.compare_with_local_library(deps);
        let needs_sync = deps_seen.len() != num_deps_to_install;
        let packages_loaded = if deps_to_remove.len() > 0 {
            get_all_packages_in_use(&self.library.path)
        } else {
            HashSet::new()
        };

        for (dir_name, notify) in &deps_to_remove {
            if packages_loaded.contains(*dir_name) {
                log::debug!(
                    "{dir_name} in the library is loaded in a session but we want to remove it."
                );
                return Err(SyncError {
                    source: SyncErrorKind::NfsError(
                        packages_loaded
                            .iter()
                            .map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
                });
            }

            // Only actually remove the deps if we are not going to do any other changes.
            if !needs_sync {
                let p = self.library.path().join(dir_name);
                if !self.dry_run && *notify {
                    log::debug!("Removing {dir_name} from library");
                    fs::remove_dir_all(&p)?;
                }

                if *notify {
                    sync_changes.push(SyncChange::removed(dir_name));
                }
            }
        }

        // If we have all the deps we need, exit early
        if !needs_sync {
            return Ok(sync_changes);
        }

        // Create staging only if we need to build stuff
        fs::create_dir_all(&self.staging_path)?;

        if let Some(log_folder) = &self.save_install_logs_in {
            fs::create_dir_all(&log_folder)?;
        }

        // Then we mark the deps seen so they won't be installed into the staging dir
        for d in &deps_seen {
            // builtin packages will not be in the library
            let in_lib = self.library.path().join(d);
            if in_lib.is_dir() {
                plan.mark_installed(*d);
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
                        let start = std::time::Instant::now();
                        let install_result = if deps_to_copy_clone.contains(dep.name.as_ref()) {
                            self.copy_package(dep)
                        } else {
                            self.install_package(dep, r_cmd, cancellation_clone.clone())
                        };

                        match install_result {
                            Ok(_) => {
                                let sync_change = SyncChange::installed(
                                    &dep.name,
                                    &dep.version.original,
                                    dep.source.clone(),
                                    dep.kind,
                                    start.elapsed(),
                                    self.system_dependencies
                                        .get(dep.name.as_ref())
                                        .cloned()
                                        .unwrap_or_default(),
                                );
                                let mut plan = plan.lock().unwrap();
                                plan.mark_installed(&dep.name);
                                drop(plan);
                                if let Some(log_folder) = &save_install_logs_in_clone {
                                    if !sync_change.is_builtin() {
                                        let log_path = sync_change.log_path(&self.cache);
                                        if log_path.exists() {
                                            fs::copy(
                                                log_path,
                                                log_folder
                                                    .join(&format!("{}.log", sync_change.name)),
                                            )
                                            .expect("no error");
                                        }
                                    }
                                }
                                if done_sender.send(sync_change).is_err() {
                                    break; // Channel closed
                                }
                            }
                            Err(e) => {
                                has_errors_clone.store(true, Ordering::Relaxed);

                                if let SyncErrorKind::InstallError(InstallError {
                                    source: InstallErrorKind::InstallationFailed(msg),
                                    ..
                                }) = &e.source
                                {
                                    if let Some(log_folder) = &save_install_logs_in_clone {
                                        fs::write(
                                            log_folder.join(&format!("{}.log", dep.name)),
                                            msg.as_bytes(),
                                        )
                                        .expect("to write files");
                                    }
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
            fs::remove_dir_all(&self.staging_path)?;
        } else {
            // If we are there, it means we are successful.

            // mv new packages to the library and delete the ones that need to be removed
            for (name, notify) in deps_to_remove {
                let p = self.library.path().join(name);
                if !self.dry_run && notify {
                    log::debug!("Removing {name} from library");
                    fs::remove_dir_all(&p)?;
                }

                if notify {
                    sync_changes.push(SyncChange::removed(name));
                }
            }

            for entry in fs::read_dir(&self.staging_path)? {
                let entry = entry?;
                let path = entry.path();
                let name = path.file_name().unwrap().to_str().unwrap().to_string();
                if !deps_seen.contains(name.as_str()) {
                    let out = self.library.path().join(&name);
                    if out.is_dir() {
                        fs::remove_dir_all(&out)?;
                    }
                    fs::rename(path, out)?;
                }
            }

            // Then delete staging
            fs::remove_dir_all(&self.staging_path)?;
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
