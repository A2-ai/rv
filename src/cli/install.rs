use crate::{
    cli::DiskCache, db::load_databases, dl_and_install_pkg, BuildPlan, BuildStep, Config,
    RCommandLine, Resolver, SystemInfo,
};
use crossbeam_channel::{unbounded, Receiver, Sender};
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
}

#[derive(Debug)]
pub enum InstallStatus {
    Success,
    Error(String),
}
/// Any extra arguments for the install command can be gathered here
pub struct InstallArgs {
    /// Destination directory where the archive will be extracted
    pub destination: PathBuf,
}

// Mock implementation of the install_pkg function
pub fn install_pkg(pkg: &str, url: &str, install_dir: &str, rvparts: &[u32; 2]) -> InstallResult {
    // Simulate installation logic
    // simulate a random sleep between 0 and 2 seconds
    // don't forget to use rand::Rng
    // let mut rng = rand::thread_rng();
    // // Generate a random number between 50 and 1000 (inclusive)
    // let sleep_duration_ms = rng.gen_range(50..=1000);
    // // Create a Duration from the random number of milliseconds
    // let sleep_duration = Duration::from_millis(sleep_duration_ms);
    // std::thread::sleep(sleep_duration);

    let outcome = dl_and_install_pkg(pkg, url, install_dir, rvparts);
    match outcome {
        Ok(_) => InstallResult {
            name: pkg.to_string(),
            status: InstallStatus::Success,
        },
        Err(e) => InstallResult {
            name: pkg.to_string(),
            status: InstallStatus::Error(e.to_string()),
        },
    }
}

pub fn execute_install(config: &Config, install_args: InstallArgs) {
    let total_start_time = std::time::Instant::now();

    // Parse config
    let r_cli = RCommandLine {};
    let no_user_override = true;

    // Determine R version
    let mut start_time = std::time::Instant::now();
    let r_version = config.get_r_version(r_cli);
    println!("time to get r version: {:?}", start_time.elapsed());

    // Determine system distribution
    start_time = std::time::Instant::now();
    let sysinfo = SystemInfo::from_os_info();
    let package_bundle_ext = match sysinfo.os_type {
        crate::OsType::MacOs => "tgz",
        crate::OsType::Windows => "zip",
        crate::OsType::Linux(_) => "tar.gz",
        crate::OsType::Other(_) => "tar.gz",
    };
    println!("time to get sysinfo: {:?}", start_time.elapsed());

    // Load databases
    start_time = std::time::Instant::now();
    let cache = DiskCache::new(&r_version, sysinfo.clone());
    let databases = load_databases(config.repositories(), &cache, &r_version, no_user_override);
    dbg!(config.repositories());
    println!("Loading databases took: {:?}", start_time.elapsed());

    // Resolve
    let resolver = Resolver::new(&databases, &r_version);
    let (resolved, unresolved) = resolver.resolve(config.dependencies());
    println!("Resolving took: {:?}", start_time.elapsed());

    if unresolved.is_empty() {
        println!("Plan successful! The following packages will be installed:");
        for d in &resolved {
            println!("    {d}");
        }
    } else {
        eprintln!("Failed to find all dependencies");
        for d in &unresolved {
            println!("    {d}");
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
            println!("Thread {}: Starting", i);
            for pkg in thread_install_receiver.iter() {
                println!("Thread {}: Starting install: {}", i, pkg.name);
                let res = install_pkg(
                    &pkg.name,
                    &pkg.url,
                    &pkg.install_dir,
                    &r_version.major_minor(),
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
                println!(
                    "sending instruction for install {:?} to {:?}",
                    &p, &install_args.destination
                );
                let repo = databases
                    .iter()
                    .find(|r| r.0.name == p.repository.to_string())
                    .map(|r| &r.0)
                    .expect("Failed to find repository for package");

                install_sender
                    .send(InstallMetadata {
                        name: p.name.to_string(),
                        url: format!(
                            "{}{}_{}.{}",
                            repo.binary_url.as_ref().unwrap(),
                            &p.name,
                            &p.version,
                            &package_bundle_ext
                        ),
                        install_dir: cache
                            .get_pkg_installation_root(&repo.url)
                            .to_string_lossy()
                            .to_string(),
                        dest_dir: install_args.destination.to_string_lossy().to_string(),
                    })
                    .expect("Failed to send install instruction");
            }
            BuildStep::Done => {
                println!("nothing to do, all done");
                break;
            }
            BuildStep::Wait => {
                println!("waiting... though shouldn't need to get here ever?");
                break;
            }
        }
    }

    println!(
        "initial packages sent to installers in {:?}",
        total_start_time.elapsed()
    );
    let iter_start_time = std::time::Instant::now();

    'outer: for result in result_receiver.iter() {
        println!("Received result for {}", result.name);
        match result.status {
            InstallStatus::Success => {
                let success_time = std::time::Instant::now();
                plan.mark_installed(&result.name);
                loop {
                    match plan.get() {
                        BuildStep::Install(p) => {
                            println!(
                                "sending instruction for install {:?} to {:?}",
                                &p, &install_args.destination
                            );
                            let repo = databases
                                .iter()
                                .find(|r| r.0.name == p.repository.to_string())
                                .map(|r| &r.0)
                                .expect("Failed to find repository for package");

                            install_sender
                                .send(InstallMetadata {
                                    name: p.name.to_string(),
                                    url: format!(
                                        "{}{}_{}.{}",
                                        repo.binary_url.as_ref().unwrap(),
                                        &p.name,
                                        &p.version,
                                        &package_bundle_ext
                                    ),
                                    install_dir: cache
                                        .get_pkg_installation_root(&repo.url)
                                        .to_string_lossy()
                                        .to_string(),
                                    dest_dir: install_args
                                        .destination
                                        .to_string_lossy()
                                        .to_string(),
                                })
                                .expect("Failed to send install instruction");
                        }
                        BuildStep::Done => {
                            println!("Total iteration time took: {:?}", iter_start_time.elapsed());
                            // no more packages to install
                            break 'outer;
                        }
                        BuildStep::Wait => {
                            break;
                        }
                    }
                }
                println!("Next step resolution took: {:?}", success_time.elapsed());
            }
            InstallStatus::Error(e) => {
                eprintln!("Failed to install {}: {}", result.name, e);
            }
        }
    }
    println!(
        "Total installation time took: {:?}",
        total_start_time.elapsed()
    );
}
