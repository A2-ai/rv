use std::{
    collections::HashMap,
    error::Error,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::Deserialize;

use crate::{
    consts::RECOMMENDED_PACKAGES,
    version::{Operator, VersionRequirement},
    RepositoryDatabase, Version,
};

// similar to crate::config, but does not return Option since Version must be present
fn deserialize_version<'de, D>(deserializer: D) -> Result<Version, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: String = Deserialize::deserialize(deserializer)?;
    match Version::from_str(&v) {
        Ok(v) => Ok(v),
        Err(_) => Err(serde::de::Error::custom("Invalid version number")),
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
// as enum since logic to resolve depends on this
enum RenvSource {
    Repository,
    GitHub,
    Local,
    Other(String),
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct PackageInfo {
    package: String,
    #[serde(deserialize_with = "deserialize_version")]
    version: Version,
    source: RenvSource,
    #[serde(default)]
    repository: Option<String>, // when source is Repository
    #[serde(default)]
    remote_type: Option<String>, // when source is GitHub
    #[serde(default)]
    remote_host: Option<String>, // when source is GitHub
    #[serde(default)]
    remote_repo: Option<String>, // when source is GitHub
    #[serde(default)]
    remote_username: Option<String>, // when source is GitHub
    #[serde(default)]
    remote_sha: Option<String>, // when source is GitHub
    #[serde(default)]
    remote_url: Option<String>, // when source is Local
    #[serde(default)]
    requirements: Vec<String>,
    hash: String,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RenvRepository {
    name: String,
    #[serde(rename = "URL")]
    url: String,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RInfo {
    #[serde(deserialize_with = "deserialize_version")]
    version: Version,
    repositories: Vec<RenvRepository>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RenvLock {
    r: RInfo,
    packages: HashMap<String, PackageInfo>,
}

impl RenvLock {
    fn parse_renv_lock<P: AsRef<Path>>(path: P) -> Result<Self, FromJsonFileError> {
        let path = path.as_ref();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                return Err(FromJsonFileError {
                    path: path.into(),
                    source: FromJsonFileErrorKind::Io(e),
                })
            }
        };

        serde_json::from_str(content.as_str()).map_err(|e| FromJsonFileError {
            path: path.into(),
            source: FromJsonFileErrorKind::Parse(e),
        })
    }

    fn resolve(
        &self,
        repository_databases: Vec<(RepositoryDatabase, bool)>,
    ) -> (Vec<ResolvedRenv>, Vec<UnresolvedRenv>) {
        let mut resolved = Vec::new();
        let mut unresolved = Vec::new();
        for (_, pkg_info) in &self.packages {
            let res = match pkg_info.source {
                RenvSource::Repository => {
                    if RECOMMENDED_PACKAGES.contains(&pkg_info.package.as_str()) {
                        continue;
                    }
                    resolve_repository(
                        &pkg_info,
                        &self.r.repositories,
                        &repository_databases,
                        &self.r.version,
                    )
                    .map(|r| Source::Repository(r))
                }
                RenvSource::GitHub => {
                    resolve_github(pkg_info).map(|(url, sha)| Source::GitHub { url, sha })
                }

                // Example package in renv.lock of Source Local
                // "rv.git.pkgA": {
                //     "Package": "rv.git.pkgA",
                //     "Version": "0.0.0.9000",
                //     "Source": "Local",
                //     "RemoteType": "local",
                //     "RemoteUrl": "src/tests/renv/rv.git.pkgA_0.0.0.9000.tar.gz",
                //     "Hash": "39e317a9ec5437bd5ce021ad56da04b6"
                // }
                RenvSource::Local => match &pkg_info.remote_url {
                    Some(path) => Ok(Source::Local(PathBuf::from(path))),
                    None => Err("Path not specified".into()),
                },
                _ => Err("Unsupported source".into()),
            };
            match res {
                Ok(source) => resolved.push(ResolvedRenv {
                    package_info: pkg_info,
                    source,
                }),
                Err(error) => unresolved.push(UnresolvedRenv {
                    package_info: pkg_info,
                    error,
                }),
            };
        }
        (resolved, unresolved)
    }
}

// Example package in renv.lock of Source Repository
// "DBI": {
//     "Package": "DBI",
//     "Version": "1.2.3",
//     "Source": "Repository",
//     "Repository": "RSPM",
//     "Requirements": [
//         "R",
//         "methods"
//     ],
//     "Hash": "065ae649b05f1ff66bb0c793107508f5"
// }
fn resolve_repository<'a>(
    pkg_info: &PackageInfo,
    repositories: &'a [RenvRepository],
    repository_databases: &[(RepositoryDatabase, bool)],
    r_version: &Version,
) -> Result<&'a RenvRepository, Box<dyn std::error::Error>> {
    // pair the repository with its database
    let repo_db_pairs = repositories
        .into_iter()
        .zip(repository_databases.into_iter())
        .map(|(r, (db, fs))| (r, db, fs))
        .collect::<Vec<_>>();

    // look for a repository corresponding with the one specified for the package
    let preferred_repo = pkg_info.repository.as_ref().and_then(|repo_name| {
        repo_db_pairs
            .iter()
            .find(|(repo, _, _)| &repo.name == repo_name)
    });

    // if there is a repository corresponding with the specified package, see if the package can be found
    if let Some((repo, repo_db, force_source)) = preferred_repo {
        if repo_db
            .find_package(
                &pkg_info.package,
                Some(&VersionRequirement::new(
                    pkg_info.version.clone(),
                    crate::version::Operator::Equal,
                )),
                r_version,
                **force_source,
            )
            .is_some()
        {
            return Ok(repo);
        }
    }

    // if the package is not found in the specified repo, look if it is in any other repo
    // sacrificing complexity for additional iter step in the repo_db_pairs instead of filtering out specified repository instance
    let secondary_repo = repo_db_pairs
        .into_iter()
        .find_map(|(repo, repo_db, force_source)| {
            repo_db.find_package(
                &pkg_info.package,
                Some(&VersionRequirement::new(
                    pkg_info.version.clone(),
                    Operator::Equal,
                )),
                r_version,
                *force_source,
            )?;

            Some(repo)
        });

    if let Some(repo) = secondary_repo {
        return Ok(repo);
    };

    Err("Not found in any repository".into())
}

// Example package in renv.lock of Source GitHub
//   "ghqc": {
//     "Package": "ghqc",
//     "Version": "0.3.2",
//     "Source": "GitHub",
//     "RemoteType": "github",
//     "RemoteHost": "api.github.com",
//     "RemoteRepo": "ghqc",
//     "RemoteUsername": "a2-ai",
//     "RemoteSha": "55c23eb6a444542dab742d3d37c7b65af7b12e38",
//     "Requirements": [
//       "R",
//       "cli",
//       "fs",
//       "glue",
//       "httpuv",
//       "rlang",
//       "rstudioapi",
//       "withr",
//       "yaml"
//     ],
//     "Hash": "dcba3cb6539ee3cfce6218049c5016cc"
//   }
fn resolve_github(pkg_info: &PackageInfo) -> Result<(String, String), Box<dyn Error>> {
    let remote_host = pkg_info
        .remote_host
        .as_deref()
        .ok_or("Missing remote host")?;
    let remote_repo = pkg_info.remote_repo.as_deref().ok_or("Missing repo")?;
    let remote_username = pkg_info
        .remote_username
        .as_deref()
        .ok_or("Missing organization/username")?;
    let remote_sha = pkg_info.remote_sha.as_deref().ok_or("Missing sha")?;

    let remote_host = remote_host
        .trim_start_matches("api.") // trim base github api
        .trim_end_matches("/api/v3"); // trim github enterprise api

    Ok((
        format!(
            "https://{}/{}/{}",
            remote_host, remote_username, remote_repo
        ),
        remote_sha.to_string(),
    ))
}

#[derive(Debug, PartialEq, Clone)]
struct ResolvedRenv<'a> {
    package_info: &'a PackageInfo,
    source: Source<'a>,
}

#[derive(Debug)]
struct UnresolvedRenv<'a> {
    package_info: &'a PackageInfo,
    error: Box<dyn std::error::Error>,
}

#[derive(Debug, PartialEq, Clone)]
enum Source<'a> {
    Repository(&'a RenvRepository),
    GitHub { url: String, sha: String },
    Local(PathBuf),
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum FromJsonFileErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] serde_json::Error),
}

#[derive(Debug, thiserror::Error)]
#[error("Error reading `{path}`")]
#[non_exhaustive]
pub struct FromJsonFileError {
    pub path: Box<Path>,
    pub source: FromJsonFileErrorKind,
}

// mod tests {
//     use crate::{
//         cli::{context::load_databases, DiskCache},
//         Repository, SystemInfo,
//     };

//     use super::RenvLock;

//     #[test]
//     fn test_renv_lock_parse() {
//         let renv_lock = RenvLock::parse_renv_lock("src/tests/renv/simple/renv.lock").unwrap();
//     }

//     #[test]
//     fn test_renv_resolve() {
//         let renv_lock = RenvLock::parse_renv_lock("src/tests/renv/multi/renv.lock").unwrap();
//         let repos = renv_lock
//             .r
//             .repositories
//             .iter()
//             .map(|r| Repository::new(r.name.to_string(), r.url.to_string(), false))
//             .collect::<Vec<_>>();
//         let cache = DiskCache::new(&renv_lock.r.version, SystemInfo::from_os_info()).unwrap();
//         let repo_db = load_databases(&repos, &cache).unwrap();
//         let (resolved, unresolved) = renv_lock.resolve(repo_db);
//         assert_eq!(resolved.len(), renv_lock.packages.len() - 2); // 1 unresolved, 1 recommended
//         assert_eq!(unresolved.len(), 1);
//     }
// }
