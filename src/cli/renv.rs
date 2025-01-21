use std::path::PathBuf;

use crate::{
    package::Package,
    renv_lock::{PackageInfo, RenvLock, RenvSource},
    Repository, RepositoryDatabase, Version,
};

fn resolve(
    renv_lock: RenvLock,
    databases: &Vec<(RepositoryDatabase, bool)>,
) -> (Vec<(Package, MigrantSource)>, Vec<PackageInfo>) {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();

    for (pkg_name, pkg_info) in &renv_lock.packages {
        // resolve based on source. Get back Package struct and info about its source
        let res = match pkg_info.source {
            RenvSource::Repository => resolve_repository(
                &pkg_name,
                &pkg_info,
                &databases,
                renv_lock.r_version(),
                renv_lock.repositories(),
            ),
            RenvSource::GitHub => resolve_github(&pkg_info),
            RenvSource::Local => resolve_local(&pkg_info),
            _ => None,
        };
        if let Some(r) = res {
            resolved.push(r);
        } else {
            unresolved.push(pkg_info.clone());
        };
    }
    (resolved, unresolved)
}

fn resolve_local(pkg_info: &PackageInfo) -> Option<(Package, MigrantSource)> {
    //verify file exists at path and return the path
    let path = PathBuf::from(&pkg_info.remote_url.as_deref()?);
    if !path.exists() {
        log::warn!(
            "Local package {} cannot be found at {}",
            pkg_info.package,
            pkg_info.remote_url.as_deref()?
        );
        return None;
    };
    let package = Package::from_renv_pkg_info(&pkg_info);
    log::debug!("{} resolved locally", pkg_info.package);
    Some((package, MigrantSource::Local(path)))
}

fn resolve_github(pkg_info: &PackageInfo) -> Option<(Package, MigrantSource)> {
    // piece together the git url as the remote_url field in PackageInfo is from the local variant
    let no_api = pkg_info.remote_host.clone()?.replace("api.", "");
    let remote = no_api.trim_end_matches("/api/v3").to_string();
    let url = format!(
        "https://{}/{}/{}",
        remote,
        pkg_info.remote_username.as_deref()?,
        pkg_info.remote_repo.as_deref()?
    );
    let package = Package::from_renv_pkg_info(&pkg_info);
    log::debug!("{} resolved to be GitHub package", pkg_info.package);
    Some((
        package,
        MigrantSource::Git {
            url,
            sha: pkg_info.remote_sha.clone()?,
        },
    ))
}

fn resolve_repository(
    pkg_name: &String,
    pkg_info: &PackageInfo,
    databases: &Vec<(RepositoryDatabase, bool)>,
    r_version: &Version,
    repos: &Vec<Repository>,
) -> Option<(Package, MigrantSource)> {
    //using vec over hashmap to maintain repository order
    let mut pkgs = Vec::new();

    // create vector of which repos contain the package
    for (repo_db, force_source) in databases {
        if let Some((pkg, _)) = repo_db.find_package(&pkg_name, None, r_version, *force_source) {
            pkgs.push((&repo_db.name, pkg));
        }
    }
    let pkg_repo = pkg_info.repository.as_deref()?;
    let (repo, pkg) =
        // see if we can find the package in the repository specified
        if let Some((repo, package)) = pkgs.iter().find(|(key, _)| key == &pkg_repo) {
            log::debug!("{} resolved to specified repository", pkg_name);
            Some((*repo, *package))
        // if not, use the first repository its found it
        } else if let Some((repo, package)) = pkgs.first() {
            log::debug!("{} resolved to a repository other than specified", pkg_name);
            Some((*repo, *package))
        // if no first, then the pkg was not found in the repositories
        } else {
            log::warn!("{} could not be found in any repository", pkg_info.package);
            return None
        }?;

    let r = repos.iter().find(|r| &r.alias == repo)?.clone();

    Some((pkg.clone(), MigrantSource::Repo(r)))
}

#[derive(Debug, Clone)]
enum MigrantSource {
    Repo(Repository),
    Git { url: String, sha: String },
    Local(PathBuf),
}

/*
//Need to mock databases for test
mod tests {
    use crate::{
        cli::{context::load_databases, DiskCache},
        SystemInfo,
    };

    use super::{resolve, RenvLock};

    #[test]
    fn resolve_renv() {
        let renv_lock = RenvLock::parse_renv_lock("src/tests/renv/renv.lock").unwrap();
        let len = renv_lock.packages.len();
        let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info()).unwrap();
        let databases = load_databases(&renv_lock.repositories(), &cache).unwrap();
        let (resolved, unresolved) = resolve(renv_lock, &databases);
        assert_eq!(unresolved.len(), 0);
        assert_eq!(resolved.len(), len);
    }
}
*/
