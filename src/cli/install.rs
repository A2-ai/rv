use crate::package::PackageType;
use crate::{
    cli::DiskCache, db::load_databases, dl_and_install_pkg, BuildPlan, BuildStep, Config,
    RCommandLine, Resolver, SystemInfo,
};
use crossbeam_channel::{unbounded, Receiver, Sender};
use log::{debug, error, info, trace}; // <-- using the log crate macros
use std::path::PathBuf;
use std::thread;

// Mock result returned by install_pkg
#[derive(Debug)]
pub struct InstallResult {
    pub name: String,
    pub status: InstallStatus,
}

#[derive(Debug)]
pub struct InstallMetadata {
    pub name: String,
    pub url: String,
    // the directory where the package should be installed
    pub install_dir: String,
    // the destination directory where the package should be available to the user
    // via a symlink from the install_dir
    pub dest_dir: String,
    pub package_type: PackageType,
}

#[derive(Debug)]
pub enum InstallStatus {
    Success,
    AlreadyPresent,
    Error(String),
}

/// Any extra arguments for the install command can be gathered here
pub struct InstallArgs {
    /// Destination directory where the archive will be extracted
    pub destination: PathBuf,
}

// Mock implementation of the install_pkg function
pub fn install_pkg(
    pkg: &str,
    url: &str,
    install_dir: &str,
    dest_dir: &str,
    rvparts: &[u32; 2],
    pkgtype: PackageType,
) -> InstallResult {
    // if package already in dest_dir, its already installed
    let dest_install = PathBuf::from(dest_dir).join(pkg);
    if dest_install.exists() {
        return InstallResult {
            name: pkg.to_string(),
            status: InstallStatus::AlreadyPresent,
        };
    }
    let installed_dir = PathBuf::from(install_dir).join(pkg);
    let outcome = dl_and_install_pkg(pkg, url, install_dir, rvparts, pkgtype, &dest_dir);
    // create symlink to dest_dir
    match outcome {
        Ok(_) => {
            trace!(
                "Creating symlink from {:?} to {:?}",
                installed_dir,
                dest_install
            );
            let link = std::os::unix::fs::symlink(&installed_dir, dest_install);
            if link.is_ok() {
                InstallResult {
                    name: pkg.to_string(),
                    status: InstallStatus::Success,
                }
            } else {
                InstallResult {
                    name: pkg.to_string(),
                    // TODO: error should specify what the explicit error was and what the linkage was attempting
                    status: InstallStatus::Error("Failed to create symlink".to_string()),
                }
            }
        }
        Err(e) => InstallResult {
            name: pkg.to_string(),
            status: InstallStatus::Error(e.to_string()),
        },
    }
}

pub fn execute_install(config: &Config, destination: &PathBuf) {
    let total_start_time = std::time::Instant::now();

    // Parse config
    let r_cli = RCommandLine {};
    let no_user_override = true;

    // Determine R version
    let mut start_time = std::time::Instant::now();
    let r_version = config.get_r_version(r_cli);
    trace!("Time to get R version: {:?}", start_time.elapsed());

    // Determine system distribution
    start_time = std::time::Instant::now();
    let sysinfo = SystemInfo::from_os_info();
    let package_bundle_ext = match sysinfo.os_type {
        crate::OsType::MacOs => "tgz",
        crate::OsType::Windows => "zip",
        crate::OsType::Linux(_) => "tar.gz",
        crate::OsType::Other(_) => "tar.gz",
    };
    trace!("Time to get SystemInfo: {:?}", start_time.elapsed());

    // Load databases
    start_time = std::time::Instant::now();
    let cache = DiskCache::new(&r_version, sysinfo.clone());
    let databases = load_databases(config.repositories(), &cache, &r_version, no_user_override);
    trace!("Repositories: {:?}", config.repositories());
    trace!("Loading databases took: {:?}", start_time.elapsed());

    // Resolve
    let resolver = Resolver::new(&databases, &r_version);
    let (resolved, unresolved) = resolver.resolve(config.dependencies());
    trace!("Resolving dependencies took: {:?}", start_time.elapsed());

    if unresolved.is_empty() {
        trace!("Plan successful! The following packages will be installed:");
        for d in &resolved {
            trace!("    {d}");
        }
    } else {
        error!("Failed to find all dependencies");
        for d in &unresolved {
            trace!("    {d}");
        }
    }

    // Create channels
    let (install_sender, install_receiver): (Sender<InstallMetadata>, Receiver<InstallMetadata>) =
        unbounded();
    let (result_sender, result_receiver): (Sender<InstallResult>, Receiver<InstallResult>) =
        unbounded();

    // Spin up worker threads
    let mut handles = Vec::new();
    let max_threads = 4;
    for i in 0..max_threads {
        let thread_install_receiver = install_receiver.clone();
        let thread_result_sender = result_sender.clone();
        let r_version = r_version.clone();
        let handle = thread::spawn(move || {
            trace!("Thread {}: Starting", i);
            for pkg in thread_install_receiver.iter() {
                trace!("Thread {}: Installing package: {}", i, pkg.name);
                let res = install_pkg(
                    &pkg.name,
                    &pkg.url,
                    &pkg.install_dir,
                    &pkg.dest_dir,
                    &r_version.major_minor(),
                    pkg.package_type,
                );
                thread_result_sender
                    .send(res)
                    .expect("Failed to send result");
            }
        });

        handles.push(handle);
    }

    // We only need one reference to our senders in the main thread
    drop(result_sender);

    // Build Plan and begin installation
    let mut plan = BuildPlan::new(&resolved);
    loop {
        match plan.get() {
            BuildStep::Install(p) => {
                trace!(
                    "Sending instruction to install {:?} to {:?}",
                    p,
                    destination
                );
                let repo = databases
                    .iter()
                    .find(|r| r.0.name == p.repository.to_string())
                    .map(|r| &r.0)
                    .expect("Failed to find repository for package");

                // if the repo.binary_url is None, we should use the src url
                let (dl_url_root, pkg_type) = match &repo.binary_url {
                    Some(url) => (url, PackageType::Binary),
                    None => (&repo.source_url.clone(), PackageType::Source),
                };
                let ext = match pkg_type {
                    PackageType::Binary => package_bundle_ext,
                    PackageType::Source => "tar.gz",
                };
                install_sender
                    .send(InstallMetadata {
                        name: p.name.to_string(),
                        url: format!("{}{}_{}.{}", dl_url_root, p.name, p.version, ext,),
                        install_dir: cache
                            .get_pkg_installation_root(&repo.url)
                            .to_string_lossy()
                            .to_string(),
                        dest_dir: destination.to_string_lossy().to_string(),
                        package_type: pkg_type,
                    })
                    .expect("Failed to send install instruction");
            }
            BuildStep::Done => {
                trace!("Nothing to do, all done.");
                break;
            }
            BuildStep::Wait => {
                trace!("Waiting... (shouldn't get here normally).");
                break;
            }
        }
    }

    let iter_start_time = std::time::Instant::now();

    // Collect results and continue building plan as they come in
    'outer: for result in result_receiver.iter() {
        match result.status {
            InstallStatus::Success | InstallStatus::AlreadyPresent => {
                plan.mark_installed(&result.name);
                loop {
                    match plan.get() {
                        BuildStep::Install(p) => {
                            trace!(
                                "Sending instruction to install {:?} to {:?}",
                                p,
                                destination
                            );
                            let repo = databases
                                .iter()
                                .find(|r| r.0.name == p.repository.to_string())
                                .map(|r| &r.0)
                                .expect("Failed to find repository for package");

                            // if the repo.binary_url is None, we should use the src url
                            let (dl_url_root, pkg_type) = match &repo.binary_url {
                                Some(url) => (url, PackageType::Binary),
                                None => (&repo.source_url.clone(), PackageType::Source),
                            };
                            let ext = match pkg_type {
                                PackageType::Binary => package_bundle_ext,
                                PackageType::Source => "tar.gz",
                            };
                            install_sender
                                .send(InstallMetadata {
                                    name: p.name.to_string(),
                                    url: format!(
                                        "{}{}_{}.{}",
                                        dl_url_root, p.name, p.version, ext,
                                    ),
                                    install_dir: cache
                                        .get_pkg_installation_root(&repo.url)
                                        .to_string_lossy()
                                        .to_string(),
                                    dest_dir: destination.to_string_lossy().to_string(),
                                    package_type: pkg_type,
                                })
                                .expect("Failed to send install instruction");
                        }
                        BuildStep::Done => {
                            debug!(
                                "done with installation iteration in: {:?}",
                                iter_start_time.elapsed()
                            );
                            // no more packages to install
                            break 'outer;
                        }
                        BuildStep::Wait => {
                            break;
                        }
                    }
                }
            }
            InstallStatus::Error(e) => {
                error!("Failed to install {}: {}", result.name, e);
            }
        }
    }

    info!(
        "Total installation time took: {:?}",
        total_start_time.elapsed()
    );
}
