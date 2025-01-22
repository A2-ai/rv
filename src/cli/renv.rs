use std::{path::PathBuf, str::FromStr};

use crate::{
    renv_lock::{PackageInfo, RenvLock, RenvRepository, RenvSource},
    version::VersionRequirement,
    RepositoryDatabase, Version,
};

/// `resolve`` takes in the Repository Databases and the parsed renv lock and determines if the package source can be determined.
/// There are three unique scenarios for resolution, determined by the Source field of the renv.lock:
/// ## Repository
/// 1. Determine which repositories the package is within
/// 2. Give priority to repository which is specified
/// 3. Take the repository that is highest in the renv.lock priority order (top to bottom)
/// 4. If not found in any repository, it is "unresolved" and communicated to the user that the output is not "full"
/// ## GitHub
/// Piece together the Url from the renv.lock components and return the Sha
/// ## Local
/// Verify the package is present in its location and return the path to the file
///
/// The function returns two vectors:
/// 1. Resolved: Each element is a tuple containing package information from the renv.lock file and source information about where the package can be found
/// 2. Unresolved: Each element is package information from the renv.lock file. For elements in this list, where the package can be sourced from cannot be found
fn resolve(
    renv_lock: RenvLock,
    databases: &Vec<(RepositoryDatabase, bool)>,
) -> (Vec<(PackageInfo, MigrantSource)>, Vec<PackageInfo>) {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();

    // separating and cloning so that renv_lock can be used directly to result in PackageInfo for returns
    let r_version = renv_lock.r_version().clone();
    let repos = renv_lock.repositories().clone();

    for (pkg_name, pkg_info) in renv_lock.packages {
        // resolve based on source. returns information based on the packages source (either a repository, the git url and sha, or path to a local file_
        let res = match pkg_info.source {
            RenvSource::Repository => {
                resolve_repository(&pkg_name, &pkg_info, &databases, &r_version, &repos)
                    .map(|repo| MigrantSource::Repo(repo))
            }
            RenvSource::GitHub => {
                resolve_github(&pkg_info).map(|(url, sha)| MigrantSource::Git { url, sha })
            }
            RenvSource::Local => resolve_local(&pkg_info).map(|path| MigrantSource::Local(path)),
            _ => None,
        };

        if let Some(r) = res {
            resolved.push((pkg_info, r));
        } else {
            unresolved.push(pkg_info);
        };
    }
    (resolved, unresolved)
}

fn resolve_local(pkg_info: &PackageInfo) -> Option<PathBuf> {
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
    log::debug!("{} resolved locally", pkg_info.package);
    Some(path)
}

fn resolve_github(pkg_info: &PackageInfo) -> Option<(String, String)> {
    // piece together the git url as the remote_url field in PackageInfo is from the local variant
    let no_api = pkg_info.remote_host.clone()?.replace("api.", "");
    let remote = no_api.trim_end_matches("/api/v3").to_string();
    let url = format!(
        "https://{}/{}/{}",
        remote,
        pkg_info.remote_username.as_deref()?,
        pkg_info.remote_repo.as_deref()?
    );
    log::debug!("{} resolved to be GitHub package", pkg_info.package);
    Some((url, pkg_info.remote_sha.clone()?))
}

fn resolve_repository(
    pkg_name: &String,
    pkg_info: &PackageInfo,
    databases: &Vec<(RepositoryDatabase, bool)>,
    r_version: &Version,
    repos: &Vec<RenvRepository>,
) -> Option<RenvRepository> {
    //using vec over hashmap to maintain repository order
    let mut pkg_repos = Vec::new();

    // create vector of which repos contain the package
    for (repo_db, force_source) in databases {
        // using existing tooling to find package among a repository database.
        let ver_req =
            VersionRequirement::from_str(&format!("(== {})", pkg_info.version.original)).ok();
        if let Some(_) = repo_db.find_package(&pkg_name, ver_req.as_ref(), r_version, *force_source)
        {
            pkg_repos.push(&repo_db.name);
        }
    }

    // check to see if the package was found in a repository as specified in the renv lock
    let pkg_repo = pkg_info.repository.as_deref()?;
    if let Some(repo_name) = pkg_repos.iter().find(|r| **r == pkg_repo) {
        log::debug!("{} resolved to specified repository", pkg_name);
        let repo = repos.iter().find(|r| r.name == **repo_name)?;
        return Some(repo.clone());
    }

    // default to the first repository the package is found in. priority based on order in renv.lock
    if let Some(repo_name) = pkg_repos.first() {
        log::debug!("{} resolved to a repository other than specified", pkg_name);
        let repo = repos.iter().find(|r| r.name == **repo_name)?;
        return Some(repo.clone());
    }

    // if not found in any repository, inform the user that manual adjustment will be needed for the package
    log::warn!(
        "{} (== {}) could not be found in any repository",
        pkg_info.package,
        pkg_info.version.original
    );
    return None;
}

#[derive(Debug, Clone)]
enum MigrantSource {
    Repo(RenvRepository),
    Git { url: String, sha: String },
    Local(PathBuf),
}

// //Need to mock databases for test
// mod tests {
//     use crate::{
//         cli::{context::load_databases, DiskCache},
//         Repository, SystemInfo,
//     };

//     use super::{resolve, RenvLock};

//     #[test]
//     fn resolve_renv() {
//         let renv_lock = RenvLock::parse_renv_lock("src/tests/renv/renv.lock").unwrap();
//         let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info()).unwrap();
//         let repositories = &renv_lock
//             .repositories()
//             .iter()
//             .map(|r| Repository {
//                 alias: r.name.clone(),
//                 url: r.url.clone(),
//                 force_source: false,
//             })
//             .collect::<Vec<Repository>>();
//         let databases = load_databases(repositories, &cache).unwrap();
//         let (resolved, unresolved) = resolve(renv_lock, &databases);
//         println!("{:#?}", unresolved);
//     }
// }
