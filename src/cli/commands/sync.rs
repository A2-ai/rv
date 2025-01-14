use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use crossbeam::{channel, thread};
use fs_err as fs;

use crate::cli::utils::untar_package;
use crate::cli::{http, link::LinkMode, CliContext};
use crate::package::PackageType;
use crate::{BuildPlan, BuildStep, RCmd, RCommandLine, RepoServer, ResolvedDependency};

fn install_package(
    context: &CliContext,
    pkg: &ResolvedDependency,
    library_dir: &Path,
) -> Result<()> {
    let link_mode = LinkMode::new();

    // If the package is already in the cache, link it directly
    if pkg.is_installed() {
        log::debug!(
            "Package {} already present in cache. Linking it in the library.",
            pkg.name
        );
        if pkg.installation_status.binary_available() {
            let binary_destination =
                context
                    .cache
                    .get_binary_package_path(pkg.repository_url, pkg.name, pkg.version);

            link_mode.link_files(&pkg.name, &binary_destination, &library_dir)?;
        }
        return Ok(());
    }

    // TODO: very similar branches
    match pkg.kind {
        PackageType::Source => {
            let destination =
                context
                    .cache
                    .get_source_package_path(pkg.repository_url, pkg.name, pkg.version);
            fs::create_dir_all(&destination)?;
            // download the file
            let mut tarball = Vec::new();
            let url = RepoServer::from_url(pkg.repository_url)
                .get_source_path(&format!("{}_{}.tar.gz", pkg.name, pkg.version));

            let bytes_read = http::download(&url, &mut tarball, vec![])?;
            // TODO: handle 404
            if bytes_read == 0 {
                bail!("Archive not found at {url}");
            }
            untar_package(Cursor::new(tarball), &destination)?;
            // run R install
            let r_cmd = RCommandLine {};
            let binary_destination =
                context
                    .cache
                    .get_binary_package_path(pkg.repository_url, pkg.name, pkg.version);
            r_cmd.install(destination.join(pkg.name), library_dir, &binary_destination)?;
            link_mode.link_files(&pkg.name, &binary_destination, &library_dir)?;
        }
        PackageType::Binary => {
            let destination =
                context
                    .cache
                    .get_binary_package_path(pkg.repository_url, pkg.name, pkg.version);
            fs::create_dir_all(&destination)?;

            // TODO: abstract all that based on repository url and thing requested
            let mut tarball = Vec::new();
            let tarball_url = RepoServer::from_url(pkg.repository_url)
                .get_binary_path(
                    &format!(
                        "{}_{}.{}",
                        pkg.name,
                        pkg.version,
                        context.cache.system_info.os_type.tarball_extension()
                    ),
                    &context.cache.r_version,
                    &context.cache.system_info,
                )
                .unwrap();
            // The return of RepoServer::get_binary_path is Some(String).
            // Able to safely unwrap here because a package's PackageType will only be Binary if its confirmed to be in the binary PACKAGE file, which returned Some from the same function

            let bytes_read = http::download(&tarball_url, &mut tarball, vec![])?;
            // TODO: handle 404
            if bytes_read == 0 {
                bail!("Archive not found at {tarball_url}");
            }

            // TODO: this might not be a binary in practice, handle that later
            untar_package(Cursor::new(tarball), &destination)?;
            link_mode.link_files(&pkg.name, &destination, &library_dir)?;
        }
    }

    Ok(())
}

#[derive(Debug)]
pub struct SyncChange {
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub timing: Option<Duration>,
}

impl SyncChange {
    pub fn new_installed(name: &str, version: &str, timing: Duration) -> Self {
        Self {
            name: name.to_string(),
            installed: true,
            timing: Some(timing),
            version: Some(version.to_string()),
        }
    }

    pub fn new_removed(name: &str) -> Self {
        Self {
            name: name.to_string(),
            installed: false,
            timing: None,
            version: None,
        }
    }
}

// sync should only display the changes made, nothing about the deps that are not changing
/// `sync` will ensure the project library contains only exactly the dependencies from rproject.toml
/// (TODO: mention lockfile later)
/// There's 2 different paths:
/// 1. All deps are already installed, we might just need to remove some
/// 2. Some deps are missing, we need to install stuff
///
/// For option 1, we can just remove the symlinks manually without a risk of breaking anything.
/// For option 2, we want to install things but we don't want to mess the current library so
/// we install everything in a temp directory and only replace the library if everything installed
/// successfully.
///
///
/// This works the following way:
/// 2. Create a temp directory if things need to be installed
/// 2. Send all dependencies to install to worker threads in order
/// 3. Once a dep is installed, get the next step until it's over or need to wait on other deps
pub fn sync(context: &CliContext, deps: Vec<ResolvedDependency>) -> Result<Vec<SyncChange>> {
    let mut sync_changes = Vec::new();
    let project_library = context.library_path();
    let plan = BuildPlan::new(&deps);
    let num_deps_to_install = plan.num_to_install();
    let deps_to_install = plan.all_dependencies_names();
    let mut to_remove = HashSet::new();
    let mut deps_seen = 0;

    if project_library.is_dir() {
        for p in fs::read_dir(&project_library)? {
            let p = p?.path().canonicalize()?;
            if p.is_dir() {
                let dir_name = p.file_name().unwrap().to_string_lossy();
                if deps_to_install.contains(&*dir_name) {
                    deps_seen += 1;
                } else {
                    to_remove.insert(dir_name.to_string());
                }
            }
        }
    }

    for dir_name in to_remove {
        // Only actually remove the deps if we are not going to rebuild the lib folder
        if deps_seen == num_deps_to_install {
            log::debug!("Removing {dir_name} from library");
            fs::remove_dir(&dir_name)?;
        }

        sync_changes.push(SyncChange::new_removed(&dir_name));
    }

    // If we have all the deps we need, exit early
    if deps_seen == num_deps_to_install {
        log::debug!("No new dependencies to install");
        return Ok(sync_changes);
    }

    // We can't use references from the BuildPlan since we borrow mutably from it so we
    // create a lookup table for resolved deps by name and use those references across channels.
    let dep_by_name: HashMap<_, _> = deps.iter().map(|d| (d.name, d)).collect();
    let plan = Arc::new(Mutex::new(plan));

    let (ready_sender, ready_receiver) = channel::unbounded();
    let (done_sender, done_receiver) = channel::unbounded();

    // Initial deps we can install immediately
    {
        let mut plan = plan.lock().unwrap();
        while let BuildStep::Install(d) = plan.get() {
            ready_sender.send(dep_by_name[d.name]).unwrap();
        }
    }

    let staging_path = context.staging_path();
    if staging_path.is_dir() {
        fs::remove_dir_all(&staging_path)?;
    }
    fs::create_dir_all(&staging_path)?;

    let installed_count = Arc::new(AtomicUsize::new(0));
    let has_errors = Arc::new(AtomicBool::new(false));
    let errors = Arc::new(Mutex::new(Vec::new()));

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
                    ready.push(dep_by_name[d.name]);
                }
                drop(plan); // Release lock before sending

                for p in ready {
                    if !seen.contains(&p.name) {
                        seen.insert(p.name);
                        ready_sender_clone.send(p).unwrap();
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            drop(ready_sender_clone);
        });

        // Our worker threads that will actually perform the installation
        // TODO: make this overridable
        let num_workers = num_cpus::get();
        for _ in 0..num_workers {
            let ready_receiver = ready_receiver.clone();
            let done_sender = done_sender.clone();
            let plan = Arc::clone(&plan);
            let has_errors_clone = Arc::clone(&has_errors);
            let errors_clone = Arc::clone(&errors);
            let s_path = staging_path.as_path();

            s.spawn(move |_| {
                while let Ok(dep) = ready_receiver.recv() {
                    if has_errors_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    log::debug!("Installing {}", dep.name);
                    let start = std::time::Instant::now();
                    match install_package(&context, dep, s_path) {
                        Ok(()) => {
                            let sync_change =
                                SyncChange::new_installed(dep.name, dep.version, start.elapsed());
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
                log::debug!(
                    "Completed installing {} ({}/{})",
                    change.name,
                    installed_count.load(Ordering::Relaxed),
                    num_deps_to_install
                );
                sync_changes.push(change);
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

    if has_errors.load(Ordering::Relaxed) {
        let err = errors.lock().unwrap();
        let mut message = String::from("Failed to install dependencies.");
        for (dep, e) in &*err {
            message += &format!("\n    Failed to install {}:\n        {e}", dep.name);
        }
        bail!(message);
    }

    // If we are there, it means we are successful. Replace the project lib by the tmp dir
    let project_library = context.library_path();
    if project_library.is_dir() {
        fs::remove_dir_all(&project_library)?;
    }
    fs::rename(&staging_path, &project_library)?;

    Ok(sync_changes)
}
