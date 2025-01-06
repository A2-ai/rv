use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::{bail, Result};
use rayon::prelude::*;

use rv::{
    cli::http,
    cli::DiskCache,
    consts::{PACKAGE_FILENAME, SOURCE_PACKAGES_PATH},
    get_binary_path, Cache, CacheEntry, Config, RCommandLine, Repository, RepositoryDatabase,
    Resolver, SystemInfo,
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
}

fn write_err(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = format!("{err}");

    let mut cause = err.source();
    while let Some(e) = cause {
        out += &format!("\nReason: {e}");
        cause = e.source();
    }

    out
}

fn load_databases(
    repositories: &[Repository],
    cache: &DiskCache,
) -> Result<Vec<(RepositoryDatabase, bool)>> {
    let dbs: Vec<std::result::Result<_, anyhow::Error>> = repositories
        .par_iter()
        .map(|r| {
            // 1. Generate path to add to URL to get the src PACKAGE and binary PACKAGE for current OS
            let entry = cache.get_package_db_entry(&r.url());
            // 2. Check in cache whether we have the database and is not expired
            match entry {
                CacheEntry::Existing(p) => {
                    // load the archive
                    let db = RepositoryDatabase::load(&p)?;
                    Ok((db, r.force_source))
                }
                CacheEntry::NotFound(p) | CacheEntry::Expired(p) => {
                    let mut db = RepositoryDatabase::new(&r.alias);
                    // download files, parse them and persist to disk
                    let mut source_package = Vec::new();
                    let source_url = format!("{}{SOURCE_PACKAGES_PATH}", r.url());
                    let bytes_read = http::download(&source_url, &mut source_package, Vec::new())?;
                    // We should ALWAYS has a PACKAGES file for source
                    if bytes_read == 0 {
                        bail!("File at {source_url} was not found");
                    }
                    // UNSAFE: we trust the PACKAGES data to be valid UTF-8
                    db.parse_source(unsafe { std::str::from_utf8_unchecked(&source_package) });

                    let mut binary_package = Vec::new();
                    let binary_path = get_binary_path(&cache.r_version, &cache.system_info);

                    let bytes_read = http::download(
                        &format!("{}{binary_path}{PACKAGE_FILENAME}", r.url()),
                        &mut binary_package,
                        vec![],
                    )?;
                    // but sometimes we might not have a binary PACKAGES file and that's fine.
                    // We only load binary if we found a file
                    if bytes_read > 0 {
                        // UNSAFE: we trust the PACKAGES data to be valid UTF-8
                        db.parse_binary(
                            unsafe { std::str::from_utf8_unchecked(&source_package) },
                            cache.r_version.clone(),
                        );
                    }

                    db.persist(&p)?;
                    println!("Saving db at {p:?}");
                    Ok((db, r.force_source))
                }
            }
            // 3. Fetch the PACKAGE files if needed and build the database + persist to disk
        })
        .collect();

    let mut res = Vec::with_capacity(dbs.len());
    let mut errs = Vec::new();
    for db in dbs {
        match db {
            Ok(d) => res.push(d),
            Err(e) => errs.push(write_err(&*e)),
        }
    }

    if !errs.is_empty() {
        bail!("Failed to load package database: {}", errs.join("\n"));
    }

    Ok(res)
}

fn try_main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Init => todo!("implement init"),
        Command::Plan => {
            let config = Config::from_file(&cli.config_file)?;
            let r_cli = RCommandLine {};
            let r_version = config.get_r_version(r_cli)?;
            let cache = DiskCache::new(&r_version, SystemInfo::from_os_info())?;
            let databases = load_databases(config.repositories(), &cache)?;

            let resolver = Resolver::new(&databases, &r_version);
            let (resolved, unresolved) = resolver.resolve(config.dependencies());

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
        }
        Command::Sync => todo!("implement sync"),
    }

    Ok(())
}

fn main() {
    if let Err(e) = try_main() {
        println!("{}", write_err(&*e));
        ::std::process::exit(1)
    }
}
