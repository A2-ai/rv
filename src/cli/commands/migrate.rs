use std::{ops::Deref, path::Path};

use anyhow::{Ok, Result};

use crate::{
    cli::{self, DiskCache}, renv_lock::{PackageInfo, RenvLock}, Repository, RepositoryDatabase, SystemInfo, Version
};

pub fn migrate() {}

fn migrate_renv<P: AsRef<Path>>(path: P) -> Result<_>{
    let renv_lock = RenvLock::parse_renv_lock(path)?;
    let resolved_deps = ResolvedLock::resolve(renv_lock);
    resolved_deps
}

#[derive(Debug, Clone)]
enum PackageSource<'a> {
    Repository(&'a Repository),
    Other(String),
}

#[derive(Debug, Clone)]
struct ResolvedLock<'a> {
    package: String,
    version: Version,
    source: PackageSource<'a>,
}

impl<'a> ResolvedLock<'a> {
    fn resolve(lock: RenvLock) -> Result<Vec<Self>> {
        let cache = DiskCache::new(lock.r_version(), SystemInfo::from_os_info())?;
        let db = cli::context::load_databases(lock.repositories(), &cache)?;
        let mut resolved_dep = Vec::new();
        for (pkg_name, pkg_info) in lock.packages.into_iter() {
            // find databases that contain the package of interest. partition based on if the repo name to give that repository priority
            let (mut matching_db, mut other_db): (Vec<_>, Vec<_>) = db.deref()
                .into_iter()
                .filter(|(repo, force_source)| repo.find_package(&pkg_name, None, &lock.r_version(), *force_source).is_some())
                .partition(|(repo, _)| repo.name == pkg_info.repository);
            // add the non-matching dbs after the matching dbs to give the matching dbs priority
            matching_db.append(&mut other_db);

            // iter through the dbs, finding which dbs contains the package
            let dependency = matching_db
                .iter()
                .map(|(repo_db, _)| Self::find_resolved(repo_db, &lock, pkg_info))
                .filter(|x| x.is_some())
                .collect::<Option<Vec<_>>>();
            
            // if the dependency was found add to the list of resolved
            if let Some(dep) = dependency {
                if let Some(first) = dep.get(0).cloned() {
                    resolved_dep.push(first);
                    continue;
                }
            }

            // if dependency not found
            
        }
        Ok(resolved_dep)
    }

    fn find_resolved(repo_db: &RepositoryDatabase, renv_lock: &'a RenvLock, pkg_info: PackageInfo) -> Option<Self> {
        let repos = renv_lock.repositories()
                                    .into_iter()
                                    .filter(|repo| repo.alias == pkg_info.repository && repo.alias == repo_db.name)
                                    .map(|x| x)
                                    .collect::<Vec<&Repository>>();
        let repo = repos.first()?;
        Some(ResolvedLock {
            package: pkg_info.package,
            version: pkg_info.version,
            source: PackageSource::Repository(repo),
        })
    }
}

mod tests {
    use super::migrate_renv;

    #[test]
    fn tester() {
        println!("{:#?}", migrate_renv("/cluster-data/user-homes/wes/projects/rv/src/tests/renv/").unwrap());
    }
}
