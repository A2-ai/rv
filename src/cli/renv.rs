use crate::cli::{context::load_databases, DiskCache};
use crate::package::Package;
use crate::renv_lock::PackageInfo;
use crate::Repository;
use crate::{renv_lock::RenvLock, SystemInfo};
use anyhow::{Ok, Result};

pub(crate) fn resolve(
    renv_lock: RenvLock,
) -> Result<(Vec<(Package, Repository)>, Vec<PackageInfo>)> {
    let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info())?;
    let db = load_databases(&renv_lock.repositories(), &cache)?;

    let mut found_pkg = Vec::new();
    let mut not_found_pkg = Vec::new();
    // loop through all packages from renv.lock file
    for (pkg_name, pkg_info) in &renv_lock.packages {
        // search for the package in all RepositoryDatabases and create a HashMap keyed on the repo name
        let mut pkgs = Vec::new();
        for (repo_db, force_source) in &db {
            if let Some((pkg, _)) =
                repo_db.find_package(&pkg_name, None, renv_lock.r_version(), *force_source)
            {
                pkgs.push((&repo_db.name, pkg.clone()));
            }
        }

        // check if we found an entry in the repository database specified by the package
        if let Some((_, pkg)) = pkgs
            .iter()
            .find(|(repo_name, _)| repo_name == &&pkg_info.repository)
        {
            if let Some(repo) = renv_lock
                .repositories()
                .iter()
                .find(|r| r.alias == pkg_info.repository)
            {
                log::debug!("{} resolved successfully", pkg.name);
                found_pkg.push((pkg.clone(), repo.clone()));
                continue;
            }
        }

        // take the first package found if not
        if let Some((repo_name, pkg)) = pkgs.first() {
            if let Some(repo) = renv_lock
                .repositories()
                .iter()
                .find(|r| &&r.alias == repo_name)
            {
                log::debug!(
                    "{} not found in specified repository, but found elsewhere",
                    pkg.name
                );
                found_pkg.push((pkg.clone(), repo.clone()));
            }
        }

        // if no entry we can't resolve
        log::warn!(
            "{} not resolved. Manual adjustment needed",
            pkg_info.package
        );
        not_found_pkg.push(pkg_info.clone());
    }
    Ok((found_pkg, not_found_pkg))
}
