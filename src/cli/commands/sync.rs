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
use crate::{get_binary_path, BuildPlan, BuildStep, RCmd, RCommandLine, ResolvedDependency};

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
            let url = format!(
                "{}/src/contrib/{}_{}.tar.gz",
                pkg.repository_url, pkg.name, pkg.version
            );

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
            let tarball_path =
                get_binary_path(&context.cache.r_version, &context.cache.system_info);
            let tarball_url = format!(
                "{}{tarball_path}{}_{}.{}",
                pkg.repository_url,
                pkg.name,
                pkg.version,
                context.cache.system_info.os_type.tarball_extension()
            );

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
/// 1. TODO: look at the current library if it exists to get the deps/versions installed
/// 2. Create a temp directory if things need to be installed
/// 2. Send all dependencies to install to worker threads in order
///     1. if the source/binary alre
pub fn sync(context: &CliContext, deps: Vec<ResolvedDependency>) -> Result<()> {
    // TODO: get the current library path
    // TODO: get the list of deps/versions installed in the current library path and
    // TODO: compare if with the `deps` argument to see if we need to install something
    // TODO: can we install things in a way that won't break the library without creating a tempdir? if it's binary only
    let plan = BuildPlan::new(&deps);
    let num_deps_to_install = plan.num_to_install();
    // TODO: too simplistic, we want to remove unneeded deps as well
    if num_deps_to_install == 0 {
        log::info!("Everything already installed");
        return Ok(());
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

    // We create a temp dir in the project dir so when/if we move it at the end we will have the right
    // link types since sometimes the tmp dir might be on another disk
    let tmp_library_dir = tempfile::tempdir_in(&context.project_dir).expect("to create a temp dir");
    let tmp_library_dir_path = tmp_library_dir.path();

    let installed_count = Arc::new(AtomicUsize::new(0));
    let has_errors = Arc::new(AtomicBool::new(false));
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

            s.spawn(move |_| {
                while let Ok(dep) = ready_receiver.recv() {
                    if has_errors_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    log::info!("Installing {}", dep.name);
                    match install_package(&context, dep, tmp_library_dir_path) {
                        Ok(()) => {
                            let mut plan = plan.lock().unwrap();
                            plan.mark_installed(&dep.name);
                            drop(plan);
                            // TODO: send timing + version
                            done_sender.send(dep.name).unwrap();
                        }
                        Err(e) => {
                            log::error!("Failed to install {}: {e}", dep.name);
                            std::thread::sleep(Duration::from_secs(1000));
                            has_errors_clone.store(true, Ordering::Relaxed);
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
            if let Ok(installed_dep) = done_receiver.recv_timeout(Duration::from_millis(1)) {
                installed_count.fetch_add(1, Ordering::Relaxed);
                log::info!(
                    "Completed installing {} ({}/{})",
                    installed_dep,
                    installed_count.load(Ordering::Relaxed),
                    num_deps_to_install
                );
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

    // If we are there, it means we are successful. Replace the project lib by the tmp dir
    let project_library = context.project_library();
    if project_library.is_dir() {
        fs::remove_dir_all(&project_library)?;
    }
    fs::rename(&tmp_library_dir_path, &project_library)?;

    // And then output all the changes that happened

    Ok(())
}
