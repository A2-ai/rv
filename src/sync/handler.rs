use crossbeam::{channel, thread};
use fs_err as fs;
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::lockfile::Source;
use crate::package::PackageType;
use crate::sync::changes::SyncChange;
use crate::sync::errors::{SyncError, SyncErrorKind, SyncErrors};
use crate::sync::{sources, LinkMode};
use crate::utils::get_max_workers;
use crate::{BuildPlan, BuildStep, DiskCache, GitExecutor, Library, RCmd, ResolvedDependency};

#[derive(Debug)]
pub struct SyncHandler<'a> {
    project_dir: &'a Path,
    library: &'a Library,
    cache: &'a DiskCache,
    staging_path: PathBuf,
    dry_run: bool,
    show_progress_bar: bool,
    max_workers: usize,
    has_lockfile: bool,
}

impl<'a> SyncHandler<'a> {
    pub fn new(
        project_dir: &'a Path,
        library: &'a Library,
        cache: &'a DiskCache,
        staging_path: impl AsRef<Path>,
    ) -> Self {
        Self {
            project_dir,
            library,
            cache,
            staging_path: staging_path.as_ref().to_path_buf(),
            dry_run: false,
            show_progress_bar: false,
            has_lockfile: false,
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

    pub fn set_has_lockfile(&mut self, has_lockfile: bool) {
        self.has_lockfile = has_lockfile;
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
    ) -> Result<(), SyncError> {
        if self.dry_run {
            return Ok(());
        }

        match dep.source {
            Source::Repository { .. } => {
                sources::repositories::install_package(dep, &self.staging_path, self.cache, r_cmd)
            }
            Source::Git { .. } => sources::git::install_package(
                dep,
                &self.staging_path,
                self.cache,
                r_cmd,
                &GitExecutor {},
            ),
            Source::Local { .. } => {
                sources::local::install_package(dep, self.project_dir, &self.staging_path, r_cmd)
            }
            Source::Url { .. } => {
                sources::url::install_package(dep, &self.staging_path, self.cache, r_cmd)
            }
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
            if deps_by_name
                .get(name.as_str())
                .map(|d| self.library.contains_package(d))
                .unwrap_or(false)
            {
                // If we don't have a lockfile, we cannot trust anything present in the library
                if self.has_lockfile {
                    deps_seen.insert(name.as_str());
                } else if matches!(deps_by_name[name.as_str()].source, Source::Local { .. }) {
                    deps_to_copy.insert(name.as_str());
                }
            } else {
                deps_to_remove.insert((name.as_str(), true));
            }
        }

        // Lastly, remove any package that we can't really access
        for name in &self.library.broken {
            log::debug!("Package {name} in library is broken");
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
        if self.staging_path.is_dir() {
            fs::remove_dir_all(&self.staging_path)?;
        }
        fs::create_dir_all(self.library.path())?;

        let mut sync_changes = Vec::new();

        let plan = BuildPlan::new(deps);
        let num_deps_to_install = plan.num_to_install();
        let (deps_seen, deps_to_copy, deps_to_remove) = self.compare_with_local_library(deps);
        let needs_sync = deps_seen.len() != num_deps_to_install;

        for (dir_name, notify) in deps_to_remove {
            // Only actually remove the deps if we are not going to rebuild the lib folder
            if !needs_sync {
                let p = self.library.path().join(dir_name);
                if !self.dry_run && notify {
                    log::debug!("Removing {dir_name} from library");
                    fs::remove_dir_all(&p)?;
                }
            }

            if notify {
                sync_changes.push(SyncChange::removed(dir_name));
            }
        }

        // If we have all the deps we need, exit early
        if !needs_sync {
            return Ok(sync_changes);
        }

        // Create staging only if we need to build stuff
        fs::create_dir_all(&self.staging_path)?;

        // We can't use references from the BuildPlan since we borrow mutably from it so we
        // create a lookup table for resolved deps by name and use those references across channels.
        let dep_by_name: HashMap<_, _> = deps.iter().map(|d| (&d.name, d)).collect();
        let pb_style =
            ProgressStyle::with_template("[{elapsed_precise}] {bar:60} {pos:>7}/{len:7}\n{msg}")
                .unwrap();

        let pb = if self.show_progress_bar {
            let pb = ProgressBar::new(plan.full_deps.len() as u64);
            pb.set_style(pb_style.clone());
            pb.enable_steady_tick(Duration::from_secs(1));
            Arc::new(pb)
        } else {
            Arc::new(ProgressBar::new(0))
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
            for _ in 0..self.max_workers {
                let ready_receiver = ready_receiver.clone();
                let done_sender = done_sender.clone();
                let plan = Arc::clone(&plan);
                let has_errors_clone = Arc::clone(&has_errors);
                let errors_clone = Arc::clone(&errors);
                let deps_to_copy_clone = Arc::clone(&deps_to_copy);
                let pb_clone = Arc::clone(&pb);
                let installing_clone = Arc::clone(&installing);

                s.spawn(move |_| {
                    while let Ok(dep) = ready_receiver.recv() {
                        if has_errors_clone.load(Ordering::Relaxed) {
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
                                    log::debug!("Installing {} (source)", dep.name)
                                }
                                PackageType::Binary => {
                                    log::debug!("Installing {} (binary)", dep.name)
                                }
                            }
                        }
                        let start = std::time::Instant::now();
                        let install_result = if deps_to_copy_clone.contains(dep.name.as_ref()) {
                            self.copy_package(dep)
                        } else {
                            self.install_package(dep, r_cmd)
                        };

                        match install_result {
                            Ok(_) => {
                                let sync_change = SyncChange::installed(
                                    &dep.name,
                                    &dep.version.original,
                                    &dep.source.to_string(),
                                    dep.kind,
                                    start.elapsed(),
                                );
                                let mut plan = plan.lock().unwrap();
                                plan.mark_installed(&dep.name);
                                drop(plan);
                                done_sender.send(sync_change).unwrap();
                            }
                            Err(e) => {
                                has_errors_clone.store(true, Ordering::Relaxed);
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
            // If we are there, it means we are successful. Replace the project lib by the staging dir
            if self.library.path().is_dir() {
                fs::remove_dir_all(self.library.path())?;
            }

            fs::rename(&self.staging_path, self.library.path())?;
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
