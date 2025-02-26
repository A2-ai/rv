//! CLI context that gets instantiated for a few commands and passed around
use std::path::{Path, PathBuf};

use crate::cli::utils::write_err;
use crate::{
    consts::LOCKFILE_NAME, find_r_version_command, get_package_file_urls,
    http, timeit, Config, DiskCache, Library, RCommandLine, Repository, RepositoryDatabase,
    SystemInfo, Version,
};

use crate::cli::utils::get_current_system_path;
use crate::consts::{RV_DIR_NAME, STAGING_DIR_NAME};
use crate::lockfile::Lockfile;
use anyhow::{anyhow, bail, Result};
use fs_err as fs;
use rayon::prelude::*;

#[derive(Debug)]
pub struct CliContext {
    pub config: Config,
    pub project_dir: PathBuf,
    pub r_version: Version,
    pub cache: DiskCache,
    pub library: Library,
    pub databases: Vec<(RepositoryDatabase, bool)>,
    pub lockfile: Option<Lockfile>,
    pub r_cmd: RCommandLine,
}

impl CliContext {
    pub fn new(config_file: &PathBuf) -> Result<Self> {
        let config = Config::from_file(config_file)?;
        let r_version = config.r_version().clone();
        let r_cmd = find_r_version_command(&r_version)
            .ok_or(anyhow!("Could not find specified version ({r_version})"))?;

        let cache = match DiskCache::new(&r_version, SystemInfo::from_os_info()) {
            Ok(c) => c,
            Err(e) => return Err(anyhow!(e)),
        };

        let project_dir = config_file.parent().unwrap().to_path_buf();
        fs::create_dir_all(project_dir.join(RV_DIR_NAME))?;
        let lockfile_path = project_dir.join(LOCKFILE_NAME);
        let lockfile = if lockfile_path.exists() {
            Some(Lockfile::load(lockfile_path)?)
        } else {
            None
        };

        let mut library = Library::new(
            &project_dir,
            get_current_system_path(&cache.system_info, r_version.major_minor()),
        );
        library.find_content();

        Ok(Self {
            config,
            cache,
            r_version,
            project_dir,
            library,
            lockfile,
            databases: Vec::new(),
            r_cmd,
        })
    }

    pub fn load_databases(&mut self) -> Result<()> {
        self.databases = load_databases(self.config.repositories(), &self.cache)?;
        Ok(())
    }

    pub fn load_databases_if_needed(&mut self) -> Result<()> {
        let can_resolve = self
            .lockfile
            .as_ref()
            .and_then(|l| Some(l.can_resolve(self.config.dependencies())))
            .unwrap_or(false);

        if !can_resolve {
            self.load_databases()?;
        }
        Ok(())
    }

    pub fn lockfile_path(&self) -> PathBuf {
        self.project_dir.join(LOCKFILE_NAME)
    }

    pub fn library_path(&self) -> &Path {
        self.library.path()
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
            let (path, exists) = cache.get_package_db_entry(&r.url());
            // 2. Check in cache whether we have the database and is not expired
            if exists {
                // load the archive
                let db = RepositoryDatabase::load(&path)?;
                log::debug!("Loaded packages db from {path:?}");
                Ok((db, r.force_source))
            } else {
                // Make sure to remove the file if it exists - it's expired
                if path.exists() {
                    fs::remove_file(&path)?;
                }
                log::debug!("Need to download PACKAGES file for {}", r.url());
                let mut db = RepositoryDatabase::new(&r.url());
                // download files, parse them and persist to disk
                let mut source_package = Vec::new();
                let (source_url, binary_url) =
                    get_package_file_urls(r.url(), &cache.r_version, &cache.system_info);
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
                log::debug!("checking for binary packages URL: {:?}", binary_url);
                // we do not know for certain that the Some return of get_binary_path will be a valid url,
                // but we do know that if it returns None there is not a binary PACKAGES file
                if let Some(url) = binary_url {
                    let bytes_read = timeit!(
                        format!("Downloaded binary PACKAGES from URL: {url}"),
                        // we can just set bytes_read to 0 if the download fails
                        // such that there is no attempt to parse the db below
                        http::download(&url, &mut binary_package, vec![],).unwrap_or(0)
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

                db.persist(&path)?;
                log::debug!("Saving packages db at {path:?}");
                Ok((db, r.force_source))
            }
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
