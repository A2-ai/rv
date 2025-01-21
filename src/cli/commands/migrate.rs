use std::path::{Path, PathBuf};

use anyhow::{Ok, Result};

use crate::{
    cli::{context::load_databases, DiskCache},
    renv_lock::{RenvLock, ResolvedLock, UnresolvedLock},
    Config, SystemInfo,
};

pub fn migrate_renv<P: AsRef<Path>>(path: P) -> Result<(Vec<ResolvedLock>, Vec<UnresolvedLock>)> {
    // get the path to the project. Independent if its the path to the renv.lock or to the project
    let path = path.as_ref();
    let path = if path.is_file() && path.file_name().is_some() {
        path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
    } else {
        path.to_path_buf()
    };
    // parse renv.lock file
    let rl_file = path.join("renv.lock");
    let renv_lock = RenvLock::parse_renv_lock(&rl_file)?;
    // resolve renv repository issues + Git + Local formatting
    let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info())?;
    let databases = load_databases(renv_lock.repositories(), &cache)?;
    Ok(renv_lock.resolve(databases))
}
