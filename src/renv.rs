use std::{collections::HashMap, error::Error, path::Path, str::FromStr};

use serde::Deserialize;

use crate::{version::{Operator, VersionRequirement}, RepositoryDatabase, Version};

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
pub(crate) enum RenvSource {
    Repository,
    GitHub,
    Local,
    Other(String),
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PackageInfo {
    pub(crate) package: String,
    #[serde(deserialize_with = "deserialize_version")]
    pub(crate) version: Version,
    pub(crate) source: RenvSource,
    #[serde(default)]
    pub(crate) repository: Option<String>, // when source is Repository
    #[serde(default)]
    remote_type: Option<String>, // when source is GitHub
    #[serde(default)]
    pub(crate) remote_host: Option<String>, // when source is GitHub
    #[serde(default)]
    pub(crate) remote_repo: Option<String>, // when source is GitHub
    #[serde(default)]
    pub(crate) remote_username: Option<String>, // when source is GitHub
    #[serde(default)]
    pub(crate) remote_sha: Option<String>, // when source is GitHub
    #[serde(default)]
    pub(crate) remote_url: Option<String>, // when source is Local
    #[serde(default)]
    pub(crate) requirements: Vec<String>,
    hash: String,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RenvRepository {
    pub(crate) name: String,
    #[serde(rename = "URL")]
    pub(crate) url: String,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RInfo {
    #[serde(deserialize_with = "deserialize_version")]
    pub(crate) version: Version,
    pub(crate) repositories: Vec<RenvRepository>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RenvLock {
    pub(crate) r: RInfo,
    pub(crate) packages: HashMap<String, PackageInfo>,
}

impl RenvLock {
    pub fn parse_renv_lock<P: AsRef<Path>>(path: P) -> Result<Self, FromJsonFileError> {
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
            match pkg_info.source {
                RenvSource::Repository => {
                    let res = resolve_repository(
                        &pkg_info,
                        &self.r.repositories,
                        &repository_databases,
                        &self.r.version,
                    );
                    match res {
                        Ok(repo) => resolved.push(ResolvedRenv {
                            package_info: pkg_info,
                            source: Source::Repository(repo),
                        }),
                        Err(e) => unresolved.push(UnresolvedRenv {
                            package_info: pkg_info,
                            error: e,
                        }),
                    }
                }
                _ => unresolved.push(UnresolvedRenv {
                    package_info: pkg_info,
                    error: "Unsupported source".into(),
                }),
            }
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
    repositories: &'a Vec<RenvRepository>,
    repository_databases: &Vec<(RepositoryDatabase, bool)>,
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
                Some(&VersionRequirement::new(pkg_info.version.clone(), Operator::Equal)),
                r_version, 
                *force_source)?;
            
            Some(repo)
        });

    if let Some(repo) = secondary_repo {
        return Ok(repo);
    };

    Err("Not found in any repository".into())
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

mod tests {
    use crate::{
        cli::{context::load_databases, DiskCache},
        Repository, SystemInfo,
    };

    use super::RenvLock;

    #[test]
    fn test_renv_lock_parse() {
        let renv_lock = RenvLock::parse_renv_lock("src/tests/renv/simple/renv.lock").unwrap();
    }

    #[test]
    fn test_renv_resolve() {
        let renv_lock = RenvLock::parse_renv_lock("src/tests/renv/simple/renv.lock").unwrap();
        let repos = renv_lock
            .r
            .repositories
            .iter()
            .map(|r| Repository::new(r.name.to_string(), r.url.to_string(), false))
            .collect::<Vec<_>>();
        let cache = DiskCache::new(&renv_lock.r.version, SystemInfo::from_os_info()).unwrap();
        let repo_db = load_databases(&repos, &cache).unwrap();
        let (resolved, unresolved) = renv_lock.resolve(repo_db);
        println!("{:#?}", resolved);
    }
}
