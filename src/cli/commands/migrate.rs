use std::path::{Path, PathBuf};

use anyhow::{Ok, Result};

use crate::{
    cli::{context::load_databases, DiskCache},
    renv_lock::RenvLock,
    Config, SystemInfo,
};

fn migrate_renv<P: AsRef<Path>>(path: P) -> Result<()> {
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
    // 
    let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info())?;
    let databases = load_databases(renv_lock.repositories(), &cache)?;
    let (resolved, _unresolved) = renv_lock.resolve(databases);

    let project_name = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|os_str| os_str.to_str())
        .map(|x| x.to_string())
        .unwrap_or("my rv project".to_string());


    /* let config = Config::resolved_lock_to_config(
        resolved,
        renv_lock.r.repositories,
        renv_lock.r.version,
        project_name,
    ); // need to write config to toml in "human readable" form
    */

    Ok(())
}
