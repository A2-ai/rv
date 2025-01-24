use std::{path::PathBuf, str::FromStr};

use crate::{
    consts::RECOMMENDED_PACKAGES, renv_lock::{PackageInfo, RenvRepository, RenvSource}, version::VersionRequirement, RepositoryDatabase, Version
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
/// The function has 4 inputs:
/// - `packages`: A hashmap containing the name of the package and then information about the package and its source
/// - `r_version`: The version of R of interest
/// - `repository_databases`: A vector of struct RenvRepositoryDatabase. The struct contains the repository, corresponding to the loaded repository database, and whether the repository is only source
/// - `ignore_recommended`: A bool indicating whether to resolve R packages with priority "recommended"
/// 
/// The function returns a vector of results, of the same order of the input `packages`, indicating either the package source or the reason the package could not be resolved
fn resolve(
    packages: Vec<PackageInfo>,
    r_version: &Version,
    repository_databases: &Vec<RenvRepositoryDatabase>,
    ignore_recommended: bool,
) -> (Vec<ResolvedRenv>, Vec<UnresolvedRenv>) {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();

    for pkg_info in packages {
        // if ignore recommended and the package is recommended or base priority. Base is not typically found in renv.lock, 
        if ignore_recommended && RECOMMENDED_PACKAGES.contains(&pkg_info.package.as_str()) {
            continue;
        }
        // resolve based on source. returns information based on the packages source (either a repository, the git url and sha, or path to a local file_
        let res = match &pkg_info.source {
            RenvSource::Repository => {
                resolve_repository(&pkg_info, &repository_databases, r_version)
                    .map(|repo| MigrantSource::Repo(repo.clone()))
            }
            RenvSource::GitHub => {
                resolve_github(&pkg_info).map(|(url, sha)| MigrantSource::Git { url, sha: sha.to_string() })
            }
            RenvSource::Local => resolve_local(&pkg_info).map(|path| MigrantSource::Local(path)),
            RenvSource::Other(source) => Err(format!("`{}` is not a supported source", source)),
        };

        match res {
            Ok(source) => resolved.push(ResolvedRenv{ pkg_info, source }),
            Err(cause) => unresolved.push(UnresolvedRenv{ pkg_info, cause }),
        }
    }
    (resolved, unresolved)
}

fn resolve_local(pkg_info: &PackageInfo) -> Result<PathBuf, String> {
    //verify file exists at path and return the path
    let path = if let Some(p) = &pkg_info.remote_url {
        PathBuf::from(p)
    } else {
        return Err("Path not specified".to_string())
    };
    if !path.exists() {
        log::warn!(
            "Local package {} cannot be found at {}",
            pkg_info.package,
            path.display()
        );
        return Err(format!("Not found at {}", path.display()));
    };
    log::debug!("{} resolved locally", pkg_info.package);
    Ok(path)
}

fn resolve_github<'a>(pkg_info: &'a PackageInfo) -> Result<(String, &'a String), String> {
    // piece together the git url as the remote_url field in PackageInfo is from the local variant
    let remote_host = pkg_info.remote_host.as_ref()
        .ok_or("Remote host not specified")?
        .replace("api.", "")
        .trim_end_matches("/api/v3")
        .to_string();
    let org = pkg_info.remote_username.as_ref().ok_or("GitHub organization not specified")?;
    let repo = pkg_info.remote_username.as_ref().ok_or("GitHub repository not specified")?;
    let url = format!(
        "https://{}/{}/{}",
        remote_host,
        org,
        repo,
    );
    let sha = pkg_info.remote_sha.as_ref().ok_or("GitHub Sha not specified")?;
    log::debug!("{} resolved to be GitHub package", pkg_info.package);
    Ok((url, sha))
}

fn resolve_repository<'a>(
    pkg_info: &PackageInfo,
    repository_databases: &Vec<RenvRepositoryDatabase<'a>>,
    r_version: &Version,
) -> Result<&'a RenvRepository, String> {
    //using vec over hashmap to maintain repository order
    let mut pkg_repos = Vec::new();

    // create vector of which repos contain the package
    for RenvRepositoryDatabase{renv_repo, repository_database, force_source} in repository_databases {
        // using existing tooling to find package among a repository database.
        let ver_req =
            VersionRequirement::from_str(&format!("(== {})", pkg_info.version.original)).ok();
        if let Some(_) = repository_database.find_package(&pkg_info.package, ver_req.as_ref(), r_version, *force_source)
        {
            pkg_repos.push(renv_repo);
        }
    }

    // check to see if the package was found in a repository as specified in the renv lock
    let pkg_repo = pkg_info.repository.as_ref().ok_or("Repository not specified")?;
    if let Some(repo) = pkg_repos.iter().find(|r| &r.name == pkg_repo) {
        log::debug!("{} resolved to specified repository", &pkg_info.package);
        return Ok(repo)
    }

    // default to the first repository the package is found in. priority based on order in renv.lock
    if let Some(repo) = pkg_repos.first() {
        log::debug!("{} resolved to a repository other than specified", &pkg_info.package);
        return Ok(repo);
    }

    // if not found in any repository, inform the user that manual adjustment will be needed for the package
    log::warn!(
        "{} (== {}) could not be found in any repository",
        pkg_info.package,
        pkg_info.version.original
    );
    return Err("Package not found in any repository".to_string());
}

struct RenvRepositoryDatabase<'a> {
    renv_repo: &'a RenvRepository,
    repository_database: RepositoryDatabase,
    force_source: bool,
}

#[derive(Debug, Clone)]
struct ResolvedRenv {
    pkg_info: PackageInfo,
    source: MigrantSource,
}

#[derive(Debug, Clone)]
struct UnresolvedRenv {
    pkg_info: PackageInfo,
    cause: String,
}

#[derive(Debug, Clone)]
enum MigrantSource {
    Repo(RenvRepository),
    Git { url: String, sha: String },
    Local(PathBuf),
}

//Need to mock databases for test
mod tests {
    use crate::{
        cli::{context::load_databases, DiskCache},
        Repository, SystemInfo, renv_lock::{RenvLock, RenvRepository},
    };

    use anyhow::Result;

    use super::{resolve, RenvRepositoryDatabase};
    
    /// this function loads the RepositoryDatabase's for a vector of RenvRepositories and returns a vector of a tuples containing RenvRepository and the loaded repository
    fn load_renv_repository_databases<'a>(repos: &'a Vec<RenvRepository>, cache: &DiskCache) -> Result<Vec<RenvRepositoryDatabase<'a>>> {
        // convert RenvRepository to Repository for loading
        let repositories = repos
            .into_iter()
            .map(|r| Repository {
                alias: r.name.to_string(),
                url: r.url.to_string(),
                force_source: false
            })
            .collect::<Vec<_>>();

        // load the RepositoryDatabase
        let dbs = load_databases(&repositories, cache)?;

        // return the RenvRepository paired with the loaded RepositoryDatabase
        Ok(
            repos
                .iter()
                .zip(dbs.into_iter())
                .map(|(repo, (repo_db, force_source)) | RenvRepositoryDatabase{renv_repo: repo, repository_database: repo_db, force_source})
                .collect::<Vec<_>>()
        )
    }

    #[test]
    fn resolve_renv() {
        let renv_lock = RenvLock::parse_renv_lock("src/tests/renv/multi/renv.lock").unwrap();
        let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info()).unwrap();

        // match RenvRepository with its RepositoryDatabase
        let databases = load_renv_repository_databases(&renv_lock.r.repositories, &cache).unwrap();

        // take only the PackageInfo since it contains the name of the package
        let packages = renv_lock.packages.into_values().collect::<Vec<_>>();

        let (resolved, unresolved) = resolve(packages, &renv_lock.r.version, &databases, true);
        println!("{:#?}", resolved);
        println!("{:#?}", unresolved);
    }
}