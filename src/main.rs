use clap::{Parser, Subcommand};
use std::path::PathBuf;

use rayon::prelude::*;

use rv::{
    cli::http,
    cli::DiskCache,
    consts::{PACKAGE_FILENAME, SOURCE_PACKAGES_PATH},
    get_binary_path, untar_package, Cache, CacheEntry, Config, RCommandLine, Repository,
    RepositoryDatabase, Resolver, SystemInfo,
};

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
    Plan,
    /// Replaces the library with exactly what is in the lock file
    Sync,
    /// Install a package
    Install {
        /// Path to the .tar.gz archive
        archive_path: PathBuf,

        /// Destination directory where the archive will be extracted
        destination: PathBuf,
    },
}

fn load_databases(repositories: &[Repository], cache: &DiskCache) -> Vec<RepositoryDatabase> {
    let dbs = repositories
        .par_iter()
        .map(|r| {
            // 1. Generate path to add to URL to get the src PACKAGE and binary PACKAGE for current OS
            let entry = cache.get_package_db_entry(&r.url());
            // 2. Check in cache whether we have the database and is not expired
            match entry {
                CacheEntry::Existing(p) => {
                    // load the archive
                    println!("Loading db at {p:?}");
                    let start_time = std::time::Instant::now();
                    let db = RepositoryDatabase::load(&p);
                    println!("Loading db took: {:?}", start_time.elapsed());
                    db
                }
                CacheEntry::NotFound(p) => {
                    let mut db = RepositoryDatabase::new(&r.alias);
                    // download files, parse them and persist to disk
                    let mut source_package = Vec::new();
                    http::download(
                        &format!("{}{SOURCE_PACKAGES_PATH}", r.url()),
                        &mut source_package,
                        None,
                    )
                    .expect("TODO");
                    // UNSAFE: we trust the PACKAGES data to be valid UTF-8
                    db.parse_source(unsafe { std::str::from_utf8_unchecked(&source_package) });

                    // TODO later
                    let mut binary_package = Vec::new();
                    let binary_path = get_binary_path(&cache.r_version, &cache.system_info);
                    // TODO: check if the downloads 404
                    http::download(
                        &format!("{}{binary_path}{PACKAGE_FILENAME}", r.url()),
                        &mut binary_package,
                        None,
                    )
                    .expect("TODO");
                    // UNSAFE: we trust the PACKAGES data to be valid UTF-8
                    db.parse_binary(
                        unsafe { std::str::from_utf8_unchecked(&source_package) },
                        cache.r_version.clone(),
                    );

                    db.persist(&p);
                    println!("Saving db at {p:?}");
                    db
                }
            }
            // 3. Fetch the PACKAGE files if needed and build the database + persist to disk
        })
        .collect::<Vec<_>>();

    dbs
}

fn try_main() {
    let cli = Cli::parse();

    // TODO: parse config file here and fetch R version if needed
    // except for init
    // let config = Config::from_file(&cli.config_file);

    match cli.command {
        Command::Install {
            archive_path,
            destination,
        } => {
            let start_time = std::time::Instant::now();
            if let Err(e) = untar_package(&archive_path, &destination) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
            println!("Package installed in {:?}", start_time.elapsed());
        }
        Command::Init => todo!("implement init"),
        Command::Plan => {
            let total_start_time = std::time::Instant::now();
            let mut start_time = std::time::Instant::now();
            let config = Config::from_file(&cli.config_file);
            let r_cli = RCommandLine {};
            let r_version = config.get_r_version(r_cli);
            let cache = DiskCache::new(&r_version, SystemInfo::from_os_info());
            start_time = std::time::Instant::now();
            let databases = load_databases(config.repositories(), &cache);
            println!("Loading databases took: {:?}", start_time.elapsed());
            start_time = std::time::Instant::now();
            let resolver = Resolver::new(&databases, &r_version);
            let (resolved, unresolved) = resolver.resolve(config.dependencies());
            println!("Resolving took: {:?}", start_time.elapsed());
            start_time = std::time::Instant::now();
            // TODO: later differentiate packages that need to be downloaded from packages
            // already cached
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
