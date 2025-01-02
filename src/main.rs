use clap::{Parser, Subcommand};
use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use rayon::prelude::*;
use rv::{
    cli::http,
    cli::DiskCache,
    consts::{PACKAGE_FILENAME, SOURCE_PACKAGES_PATH},
    dl_and_install_pkg, get_binary_path, BuildPlan, BuildStep, Cache, CacheEntry, Config,
    RCommandLine, Repository, RepositoryDatabase, Resolver, SystemInfo, Version,
};
use std::time::Duration;
use std::{path::Path, thread};
use std::{path::PathBuf, str::FromStr};
// use rand::Rng;
#[derive(Parser)]
#[clap(version, author, about, subcommand_negates_reqs = true)]
pub struct Cli {
    /// Do not print any output
    #[clap(long, default_value_t = false)]
    pub quiet: bool,

    /// Path to a config file other than rproject.toml in the current directory
    #[clap(short = 'c', long, default_value = "rproject.toml")]
    pub config_file: PathBuf,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Creates a new rv project
    Init,
    /// Dry run of what sync would do
    Plan {
        /// Specify the R version (e.g., 4.3, 4.4.1)
        #[clap(long, value_parser)]
        r_version: Option<String>,

        /// Specify the system distribution (e.g., jammy, mac)
        #[clap(long, value_enum)]
        distribution: Option<Distribution>,
    },
    /// Replaces the library with exactly what is in the lock file
    Sync,
    /// Install a package
    Install {
        /// Destination directory where the archive will be extracted
        destination: PathBuf,
    },
}
use clap::ValueEnum;

#[derive(Debug, Clone, PartialEq, Eq, ValueEnum)]
pub enum Distribution {
    Mac,
    Windows,
    Focal,
    Jammy,
    Noble,
}
fn load_databases(
    repositories: &[Repository],
    cache: &DiskCache,
    r_version: &Version,
    persist: bool,
) -> Vec<(RepositoryDatabase, bool)> {
    let dbs = repositories
        .par_iter()
        .map(|r| {
            // 1. Generate path to add to URL to get the src PACKAGE and binary PACKAGE for current OS
            let entry = cache.get_package_db_entry(&r.url());
            // 2. Check in cache whether we have the database and is not expired
            match entry {
                CacheEntry::Existing(p) => {
                    // load the archive
                    println!("Loading db from cache {p:?}");
                    let start_time = std::time::Instant::now();
                    let db = RepositoryDatabase::load(&p);
                    println!("Loading db from cache took: {:?}", start_time.elapsed());
                    (db, r.force_source)
                }
                CacheEntry::NotFound(p) => {
                    // TODO: get the repository db responsibilities refactored to not track binary/source packages
                    let mut db = RepositoryDatabase::new(&r.alias);
                    // download files, parse them and persist to disk
                    db.url = r.url().to_string();
                    let mut source_package = Vec::new();
                    let mut start_time = std::time::Instant::now();
                    http::download(
                        &format!("{}{SOURCE_PACKAGES_PATH}", r.url()),
                        &mut source_package,
                        None,
                    )
                    .expect("TODO");
                    db.source_url = format!(
                        "{}{}",
                        r.url(),
                        std::path::Path::new(SOURCE_PACKAGES_PATH)
                            .parent()
                            .unwrap()
                            .to_string_lossy()
                    );
                    println!(
                        "Downloading source package took: {:?}",
                        start_time.elapsed()
                    );
                    start_time = std::time::Instant::now();
                    // UNSAFE: we trust the PACKAGES data to be valid UTF-8
                    db.parse_source(unsafe { std::str::from_utf8_unchecked(&source_package) });
                    println!("Parsing source package took: {:?}", start_time.elapsed());
                    // TODO later
                    let mut binary_package = Vec::new();
                    let binary_path = get_binary_path(
                        &cache.r_version,
                        &cache.system_info.os_type,
                        cache.system_info.codename(),
                    );
                    let binary_path = format!("{}{binary_path}", r.url());
                    let dl_url = format!("{}{PACKAGE_FILENAME}", binary_path);
                    println!("Downloading binary package from {dl_url}");
                    start_time = std::time::Instant::now();
                    // TODO: check if the downloads 404
                    let rvparts = r_version.major_minor();
                    http::download(
                        &dl_url,
                        &mut binary_package,
                        Some((
                            "user-agent",
                            format!("R/{}.{}", rvparts[0], rvparts[1]).into(),
                        )),
                    )
                    .expect("TODO");
                    // TODO: set binary URL only if successfully able to dl packages to represent binaries should be available
                    db.binary_url = Some(binary_path);
                    println!(
                        "Downloading binary package took: {:?}",
                        start_time.elapsed()
                    );
                    // UNSAFE: we trust the PACKAGES data to be valid UTF-8
                    start_time = std::time::Instant::now();
                    db.parse_binary(
                        unsafe { std::str::from_utf8_unchecked(&source_package) },
                        cache.r_version.clone(),
                    );
                    println!("Parsing binary package took: {:?}", start_time.elapsed());
                    start_time = std::time::Instant::now();
                    // we may only want to cache/persist the db for our own platform where
                    // we make repeated calls. realistically I just don't know the implementation
                    // of the db storage/persistence and don't want the different platform queries
                    // to cause problems in the db persistence so am keeping this as an escape hatch
                    // while experimenting
                    if persist {
                        db.persist(&p);
                    }
                    println!("Persisting db took: {:?}", start_time.elapsed());
                    println!("Saving db at {p:?}");
                    (db, r.force_source)
                }
            }
            // 3. Fetch the PACKAGE files if needed and build the database + persist to disk
        })
        .collect::<Vec<_>>();

    dbs
}

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

fn try_main() {
    let cli = Cli::parse();

    // TODO: parse config file here and fetch R version if needed
    // except for init
    // let config = Config::from_file(&cli.config_file);

    match cli.command {
        Command::Install { destination } => {
            let total_start_time = std::time::Instant::now();
            let config = Config::from_file(&cli.config_file);
            let r_cli = RCommandLine {};
            // only for planning simulation, so for install we can't install other platforms
            let no_user_override = true;
            // Determine the R version
            let mut start_time = std::time::Instant::now();
            let r_version = config.get_r_version(r_cli);
            println!("time to get r version: {:?}", start_time.elapsed());
            start_time = std::time::Instant::now();
            // Determine the distribution and set up SystemInfo
            let sysinfo = SystemInfo::from_os_info(); // Fallback to system detection
            println!("time to get sysinfo: {:?}", start_time.elapsed());
            start_time = std::time::Instant::now();
            let cache = DiskCache::new(&r_version, sysinfo.clone());
            let databases = load_databases(
                config.repositories(),
                &cache,
                &r_version,
                no_user_override, // only persist if no override
            );
            dbg!(config.repositories());
            println!("Loading databases took: {:?}", start_time.elapsed());

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
            // Create channels for distributing URLs and collecting results
            let (install_sender, install_receiver): (
                Sender<InstallMetadata>,
                Receiver<InstallMetadata>,
            ) = unbounded();
            let (result_sender, result_receiver): (Sender<InstallResult>, Receiver<InstallResult>) =
                unbounded();
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

            // Drop the extra senders in the main thread
            drop(result_sender);

            let mut plan = BuildPlan::new(&resolved);
            loop {
                match plan.get() {
                    BuildStep::Install(p) => {
                        // install the package
                        // untar_package(&archive_path, &destination).expect("Failed to install package");
                        println!(
                            "sending instruction for install {:?} to {:?}",
                            &p, &destination
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
                                    "{}{}_{}.tgz",
                                    repo.binary_url.as_ref().unwrap(),
                                    &p.name,
                                    &p.version
                                ),
                                install_dir: cache
                                    .get_pkg_installation_root(&repo.url)
                                    .to_string_lossy()
                                    .to_string(),
                                dest_dir: destination.to_string_lossy().to_string(),
                            })
                            .expect("Failed to send install instruction");
                    }
                    BuildStep::Done => {
                        // wait for the package to be installed
                        println!("nothing to do, all done");
                    }
                    BuildStep::Wait => {
                        // wait for the package to be installed
                        println!("waiting... though shouldn't need to get here ever?");
                        break;
                    }
                }
            }
            println!("initial packages sent to installers in {:?}", total_start_time.elapsed());
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
                                    // install the package
                                    // untar_package(&archive_path, &destination).expect("Failed to install package");
                                    println!(
                                        "sending instruction for install {:?} to {:?}",
                                        &p, &destination
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
                                                "{}{}_{}.tgz",
                                                repo.binary_url.as_ref().unwrap(),
                                                &p.name,
                                                &p.version
                                            ),
                                            install_dir: cache
                                                .get_pkg_installation_root(&repo.url)
                                                .to_string_lossy()
                                                .to_string(),
                                            dest_dir: destination.to_string_lossy().to_string(),
                                        })
                                        .expect("Failed to send install instruction");
                                }
                                BuildStep::Done => {
                                    println!(
                                        "Total iteration time took: {:?}",
                                        iter_start_time.elapsed()
                                    );
                                    // no more packages to install!
                                    break 'outer;
                                }
                                BuildStep::Wait => {
                                    // wait for the package to be installed
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
        Command::Init => todo!("implement init"),
        Command::Plan {
            r_version,
            distribution,
        } => {
            let total_start_time = std::time::Instant::now();
            let config = Config::from_file(&cli.config_file);
            let r_cli = RCommandLine {};
            let no_user_override = r_version.is_none() && distribution.is_none();
            // Determine the R version
            let mut start_time = std::time::Instant::now();
            let r_version = match r_version {
                Some(ver_str) => Version::from_str(&ver_str).expect("Invalid R version format"),
                None => config.get_r_version(r_cli),
            };
            println!("time to get r version: {:?}", start_time.elapsed());
            start_time = std::time::Instant::now();
            // Determine the distribution and set up SystemInfo
            let sysinfo = match distribution {
                Some(Distribution::Mac) => SystemInfo::new(
                    rv::OsType::MacOs,
                    // should allow this to be specified at some point, but for now we only use arm macs
                    // so will expect that be the core need for now
                    Some("aarch64".into()),
                    None,
                    // this isn't really used right now
                    "12.0",
                ),
                Some(Distribution::Windows) => SystemInfo::new(
                    rv::OsType::Windows,
                    // no arm windows support yet so no point
                    Some("x86_64".into()),
                    None,
                    // this isn't really used right now
                    "12.0",
                ),
                Some(dist) => {
                    // Handle Linux distributions
                    let (os_type, codename, release) = match dist {
                        Distribution::Focal => ("ubuntu", "focal", "20.04"),
                        Distribution::Jammy => ("ubuntu", "jammy", "22.04"),
                        Distribution::Noble => ("ubuntu", "noble", "24.04"),
                        _ => unreachable!(), // Already handled Mac and Windows
                    };
                    SystemInfo::new(
                        rv::OsType::Linux(os_type),
                        // no arm linux support yet so no point
                        Some("x86_64".into()),
                        Some(codename.to_string()),
                        release,
                    )
                }
                None => SystemInfo::from_os_info(), // Fallback to system detection
            };
            println!("time to get sysinfo: {:?}", start_time.elapsed());
            start_time = std::time::Instant::now();
            let cache = DiskCache::new(&r_version, sysinfo.clone());
            let databases = load_databases(
                config.repositories(),
                &cache,
                &r_version,
                no_user_override, // only persist if no override
            );
            println!("Loading databases took: {:?}", start_time.elapsed());

            let resolver = Resolver::new(&databases, &r_version);
            let (resolved, unresolved) = resolver.resolve(config.dependencies());
            println!("Resolving took: {:?}", start_time.elapsed());

            if unresolved.is_empty() {
                println!("Plan successful! The following packages will be installed:");
                for d in resolved {
                    println!("    {d}");
                }
            } else {
                eprintln!("Failed to find all dependencies");
                for d in unresolved {
                    println!("    {d}");
                }
            }
            println!("Plan took: {:?}", total_start_time.elapsed());
        }
        Command::Sync => todo!("implement sync"),
    }
}

fn main() {
    try_main()
}
