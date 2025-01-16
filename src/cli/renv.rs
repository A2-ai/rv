use std::collections::HashMap;

use crate::{
    cli::context::load_databases,
    package::Package,
    renv_lock::{PackageInfo, RenvLock},
    Repository, SystemInfo,
};
use anyhow::{Ok, Result};

use super::DiskCache;

pub(crate) struct ResolvedRenv {
    package: Package,
    repository: Repository,
}

impl ResolvedRenv {
    pub(crate) fn resolve_renv(renv_lock: RenvLock) -> Result<(Vec<Self>, Vec<PackageInfo>)> {
        // logic used: HashMap of (Repository, Option<RepositoryDatabase>) and HashMap of (PackageInfo, Option<Repository>)
        // look through the packages that have `Some` repository and try to find it in the RepositoryDatabase
        // if not found in the specified repositories database, add it to the list of packages that had `None` repository
        // look through all of the repos with `Some` repository database (in order) to try to find the package
        // if found in a different repo, we say its fine
        // if not found in any repo, warn the user
        
        let r_version = renv_lock.r_version().clone();

        
        let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info())?;
        let db = load_databases(renv_lock.repositories(), &cache)?;
        let mut hash_db = HashMap::new();
        for repo in renv_lock.r.repositories.clone() {
            let repo_db = db
                .clone()
                .into_iter()
                .find((|(rdb, _)| rdb.name == repo.alias));
            hash_db.insert(repo, repo_db);
        }

        let (found_repo, mut not_found_repo): (
            HashMap<PackageInfo, Option<Repository>>,
            HashMap<PackageInfo, Option<Repository>>,
        ) = pkg_repo(renv_lock)
            .into_iter()
            .partition(|(_, repo)| repo.is_some());

        let mut found_pkg = Vec::new();
        for (pkg_info, repo) in found_repo {
            if let Some(repository) = repo {
                if let Some(Some((repo_db, force_source))) = hash_db.get(&repository) {
                    if let Some((package, _)) =
                        repo_db.find_package(&pkg_info.package, None, &r_version, *force_source)
                    {
                        log::debug!("{} found in specified repo", package.name);
                        found_pkg.push(ResolvedRenv {
                            package: package.clone(),
                            repository: repository.clone(),
                        });
                        continue;
                    }
                }
            }
            not_found_repo.insert(pkg_info, None);
        }

        let mut not_found_pkg = Vec::new();
        for (pkg_info, _) in not_found_repo {
            let mut flag = true;
            for (repo, d) in hash_db.clone() {
                if let Some((repo_db, force_source)) = d {
                    if let Some((package, _)) =
                        repo_db.find_package(&pkg_info.package, None, &r_version, force_source)
                    {
                        log::debug!(
                            "{} not found in specified repo. Found in {}",
                            package.name,
                            repo.url()
                        );
                        found_pkg.push(ResolvedRenv {
                            package: package.clone(),
                            repository: repo,
                        });
                        flag = false;
                        continue;
                    }
                }
            }
            if flag {
                log::warn!("{} not found in any specified repository", pkg_info.package);
                not_found_pkg.push(pkg_info);
            }
        }

        Ok((found_pkg, not_found_pkg))
    }
}

fn pkg_repo(renv_lock: RenvLock) -> HashMap<PackageInfo, Option<Repository>> {
    let mut results = HashMap::new();
    for (_, pkg_info) in renv_lock.packages {
        let repo = renv_lock
            .r
            .repositories
            .clone()
            .into_iter()
            .find(|r| r.alias == pkg_info.repository);
        results.insert(pkg_info.clone(), repo.clone());
    }
    results
}
