//! CLI context that gets instantiated for a few commands and passed around
use std::path::PathBuf;

use crate::cli::{http, utils::write_err, DiskCache};
use crate::{
    consts::LOCKFILE_NAME, consts::PACKAGE_FILENAME, timeit, Cache, CacheEntry, Config,
    RCommandLine, RepoServer, Repository, RepositoryDatabase, SystemInfo, Version,
};

use crate::cli::utils::get_os_path;
use crate::lockfile::Lockfile;
use anyhow::{bail, Result};
use fs_err as fs;
use rayon::prelude::*;

const RV_DIR_NAME: &str = "rv";
const LIBRARY_DIR_NAME: &str = "library";
const STAGING_DIR_NAME: &str = "staging";

#[derive(Debug)]
pub struct CliContext {
    pub config: Config,
    pub project_dir: PathBuf,
    pub r_version: Version,
    pub cache: DiskCache,
    pub databases: Vec<(RepositoryDatabase, bool)>,
    pub lockfile: Option<Lockfile>,
}

impl CliContext {
    pub fn new(config_file: &PathBuf) -> Result<Self> {
        let config = Config::from_file(config_file)?;
        let r_cli = RCommandLine {};
        let r_version = config.get_r_version(r_cli)?;

        let cache = DiskCache::new(&r_version, SystemInfo::from_os_info())?;

        let project_dir = config_file.parent().unwrap().to_path_buf();
        fs::create_dir_all(project_dir.join(RV_DIR_NAME))?;
        let lockfile_path = project_dir.join(LOCKFILE_NAME);
        let lockfile = if lockfile_path.exists() {
            Some(Lockfile::load(lockfile_path)?)
        } else {
            None
        };

        Ok(Self {
            config,
            cache,
            r_version,
            project_dir,
            lockfile,
            databases: Vec::new(),
        })
    }

    pub fn load_databases(&mut self) -> Result<()> {
        self.databases = load_databases(self.config.repositories(), &self.cache)?;
        Ok(())
    }

    pub fn lockfile_path(&self) -> PathBuf {
        self.project_dir.join(LOCKFILE_NAME)
    }

    pub fn library_path(&self) -> PathBuf {
        self.project_dir
            .join(RV_DIR_NAME)
            .join(LIBRARY_DIR_NAME)
            .join(get_os_path(
                &self.cache.system_info,
                self.r_version.major_minor(),
            ))
    }

    pub fn staging_path(&self) -> PathBuf {
        self.project_dir.join(RV_DIR_NAME).join(STAGING_DIR_NAME)
    }
}

pub(crate) fn load_databases(
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
                    // Make sure to remove the file if it exists - it's expired
                    if p.exists() {
                        fs::remove_file(&p)?;
                    }
                    log::debug!("Need to download PACKAGES file for {}", r.url());
                    let mut db = RepositoryDatabase::new(&r.alias, &r.url());
                    // download files, parse them and persist to disk
                    let mut source_package = Vec::new();
                    let source_url =
                        RepoServer::from_url(r.url()).get_source_path(PACKAGE_FILENAME);
                    let bytes_read = timeit!(
                        "Downloaded source PACKAGES",
                        http::download(&source_url, &mut source_package, Vec::new())?
                    );
                    // We should ALWAYS has a PACKAGES file for source
                    if bytes_read == 0 {
                        bail!("File at {source_url} was not found");
                    }
                    // UNSAFE: we trust the PACKAGES data to be valid UTF-8
                    db.parse_source(unsafe { std::str::from_utf8_unchecked(&source_package) });

                    let mut binary_package = Vec::new();
                    let binary_url = RepoServer::from_url(r.url()).get_binary_path(
                        PACKAGE_FILENAME,
                        &cache.r_version,
                        &cache.system_info,
                    );

                    // we do not know for certain that the Some return of get_binary_path will be a valid url,
                    // but we do know that if it returns None there is not a binary PACKAGES file
                    if let Some(url) = binary_url {
                        let bytes_read = timeit!(
                            "Downloaded binary PACKAGES",
                            http::download(&url, &mut binary_package, vec![],)?
                        );
                        // but sometimes we might not have a binary PACKAGES file and that's fine.
                        // We only load binary if we found a file
                        if bytes_read > 0 {
                            // UNSAFE: we trust the PACKAGES data to be valid UTF-8
                            db.parse_binary(
                                unsafe { std::str::from_utf8_unchecked(&binary_package) },
                                cache.r_version.clone(),
                            );
                        }
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
