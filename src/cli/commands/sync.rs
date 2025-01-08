// TODO
// 1. List packages installed in current library
// 2. Make the plan, it should check whether the version we get is already available in the cache
// 3. Make a list of packages to be removed that are not in the plan
// 4. Create a temp dir and install all packages in there
// 5. Replace the contents of the library path with that temp dir

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Result};
use crossbeam::{channel, thread};

use crate::cache::InstallationStatus;
use crate::cli::utils::{create_dir_all, untar_package};
use crate::cli::{http, CliContext};
use crate::package::PackageType;
use crate::{get_binary_path, BuildPlan, BuildStep, ResolvedDependency};

fn install_package(context: &CliContext, pkg: &ResolvedDependency) -> Result<()> {
    bail!("Teting errors");
    match pkg.kind {
        PackageType::Source => {
            let destination =
                context
                    .cache
                    .get_source_package_path(pkg.repository_url, pkg.name, pkg.version);
            create_dir_all(&destination)?;
            // download the file
            // run R install
        }
        PackageType::Binary => {
            let destination =
                context
                    .cache
                    .get_binary_package_path(pkg.repository_url, pkg.name, pkg.version);
            create_dir_all(&destination)?;

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
            untar_package(Cursor::new(tarball), destination)?;
        }
    }

    Ok(())
}

// sync should only display the changes made, nothing about the deps that are not changing
pub fn sync(context: &CliContext, deps: Vec<ResolvedDependency>) -> Result<()> {
    // TODO: find the current library path

    // We can't use references from the BuildPlan since we borrow mutably from it so we
    // create a lookup table for resolved deps by name and use those references across channels.
    let dep_by_name: HashMap<_, _> = deps.iter().map(|d| (d.name, d)).collect();

    let mut plan = BuildPlan::new(&deps);
    // for d in &deps {
    //     if d.installation_status != InstallationStatus::Absent {
    //         plan.mark_installed(&d.name);
    //     }
    // }

    let num_deps_to_install = plan.num_to_install();
    if num_deps_to_install == 0 {
        log::info!("Everything already installed");
        return Ok(());
    }

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
                        return;
                    }

                    // Simulate installation
                    log::info!("Installing {}", dep.name);
                    match install_package(&context, dep) {
                        Ok(()) => {
                            let mut plan = plan.lock().unwrap();
                            plan.mark_installed(&dep.name);
                            drop(plan);
                            // TODO: send timing + version
                            done_sender.send(dep.name).unwrap();
                        }
                        Err(e) => {
                            log::error!("Failed to install {}: {e}", dep.name);
                            has_errors_clone.store(true, Ordering::Relaxed);
                        }
                    }
                }
            });
        }

        // let mut result = Vec::new();
        // Monitor progress in the main thread
        while !has_errors.load(Ordering::Relaxed)
            && installed_count.load(Ordering::Relaxed) < num_deps_to_install
        {
            if let Ok(installed_dep) = done_receiver.recv() {
                installed_count.fetch_add(1, Ordering::Relaxed);
                log::info!(
                    "Completed installing {} ({}/{})",
                    installed_dep,
                    installed_count.load(Ordering::Relaxed),
                    num_deps_to_install
                );
            }
        }

        // Clean up
        drop(ready_sender);
    })
    .expect("threads to not panic");

    Ok(())
}

// TODO:
// 2. source install (create a library in a temp dir for the install)
// 3. logging + timing
