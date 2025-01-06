//! CLI context that gets instantiated for a few commands and passed around
use std::path::PathBuf;

use crate::cli::{http, utils::write_err, DiskCache};
use crate::{
    consts::{PACKAGE_FILENAME, SOURCE_PACKAGES_PATH},
    get_binary_path, Cache, CacheEntry, Config, RCommandLine, Repository, RepositoryDatabase,
    SystemInfo, Version,
};

use anyhow::{bail, Result};
use rayon::prelude::*;

#[derive(Debug)]
pub struct CliContext {
    pub config: Config,
    pub r_version: Version,
    pub cache: DiskCache,
    pub databases: Vec<(RepositoryDatabase, bool)>,
}

impl CliContext {
    pub fn new(config_file: &PathBuf) -> Result<Self> {
        let config = Config::from_file(config_file)?;
        let r_cli = RCommandLine {};
        let r_version = config.get_r_version(r_cli)?;
        let cache = DiskCache::new(&r_version, SystemInfo::from_os_info())?;
        let databases = load_databases(config.repositories(), &cache)?;

        Ok(Self {
            config,
            cache,
            databases,
            r_version,
        })
    }
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
                    log::debug!("Loaded packages db from {p:?}");
                    Ok((db, r.force_source))
                }
                CacheEntry::NotFound(p) | CacheEntry::Expired(p) => {
                    let mut db = RepositoryDatabase::new(&r.alias, &r.url());
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
                    log::debug!("Saving packages db at {p:?}");
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
