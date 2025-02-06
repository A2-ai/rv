use std::{collections::HashMap, error::Error, path::{Path, PathBuf}};

use serde::Deserialize;

use crate::{
    package::{deserialize_version, Operator, Version, VersionRequirement},
    RepositoryDatabase,
};

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
        repository_database: &[(RepositoryDatabase, bool)],
    ) -> (Vec<ResolvedRenv>, Vec<UnresolvedRenv>) {
        let mut resolved = Vec::new();
        let mut unresolved = Vec::new();
        for (_, package_info) in &self.packages {
            let res = match package_info.source {
                RenvSource::Repository => resolve_repository(
                    package_info,
                    &self.r.repositories,
                    repository_database,
                    &self.r.version,
                )
                .map(|r| Source::Repository(r)),
                RenvSource::GitHub => resolve_github(package_info)
                    .map(|(git, sha)| Source::GitHub {git, sha}),
                RenvSource::Local => resolve_local(package_info)
                    .map(|path| Source::Local(path)),
                _ => Err("Source is not supported".into()),
            };
            match res {
                Ok(source) => resolved.push(ResolvedRenv {
                    package_info,
                    source,
                }),
                Err(error) => unresolved.push(UnresolvedRenv {
                    package_info,
                    error,
                }),
            }
        }
        (resolved, unresolved)
    }
}

// Expected Repository sourced package format from renv.lock
// "R6": {
//     "Package": "R6",
//     "Version": "2.5.1",
//     "Source": "Repository",
//     "Repository": "RSPM",
//     "Requirements": [
//     "R"
//     ],
//     "Hash": "470851b6d5d0ac559e9d01bb352b4021"
// },
fn resolve_repository<'a>(
    pkg_info: &PackageInfo,
    repositories: &'a [RenvRepository],
    repository_database: &[(RepositoryDatabase, bool)],
    r_version: &Version,
) -> Result<&'a RenvRepository, Box<dyn Error>> {
    let version_requirement = VersionRequirement::new(pkg_info.version.clone(), Operator::Equal);

    // match the repository database with its corresponding repository
    let repo_pairs = repositories
        .iter()
        .zip(repository_database.into_iter())
        .map(|(repo, (repo_db, force_source))| (repo, repo_db, force_source))
        .collect::<Vec<_>>();

    // if a repository is found as that is specified by the package log, look in it first
    let pref_repo_pair = pkg_info
        .repository
        .as_ref()
        .and_then(|repo_name| repo_pairs.iter().find(|(r, _, _)| &r.name == repo_name));
    if let Some((repo, repo_db, force_source)) = pref_repo_pair {
        if repo_db
            .find_package(
                &pkg_info.package,
                Some(&version_requirement),
                r_version,
                **force_source,
            )
            .is_some()
        {
            return Ok(repo);
        };
    }

    // if a repository is not found in its specified repository, look in the rest of the repositories
    // sacrificing one additional iteration step of re-looking up in preferred repository for less complexity
    repo_pairs
        .into_iter()
        .find_map(|(repo, repo_db, force_source)| {
            repo_db.find_package(
                &pkg_info.package,
                Some(&version_requirement),
                r_version,
                *force_source,
            )?;
            Some(repo)
        })
        .ok_or("Could not find package in repository".into())
}


// Expected GitHub sourced package format from renv.lock
// "ghqc": {
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
fn resolve_github(pkg_info: &PackageInfo) -> Result<(String, String), Box<dyn Error>> {
    let host = pkg_info.remote_host.as_ref().ok_or("RemoteHost not found")?;
    let repo = pkg_info.remote_repo.as_ref().ok_or("RemoteRepo not found")?;
    let org = &pkg_info.remote_username.as_ref().ok_or("RemoteUsername not found")?;
    let sha = &pkg_info.remote_sha.as_ref().ok_or("RemoteSha not found")?;
    let base_url = host.trim_start_matches("https://").trim_start_matches("api.").trim_end_matches("api/v3");
    let url = format!("https://{base_url}/{org}/{repo}");
    Ok((url, sha.to_string()))
}

// Expected local sourced package, installed via renv::install, format from renv.lock
// "rv.git.pkgA": {
//       "Package": "rv.git.pkgA",
//       "Version": "0.0.0.9000",
//       "Source": "Local",
//       "RemoteType": "local",
//       "RemoteUrl": "~/projects/rv.git.pkgA_0.0.0.9000.tar.gz",
//       "Hash": "39e317a9ec5437bd5ce021ad56da04b6"
//     }
fn resolve_local(pkg_info: &PackageInfo) -> Result<PathBuf, Box<dyn Error>> {
    let path = pkg_info.remote_url.as_ref().ok_or("RemoteUrl not found")?;
    Ok(PathBuf::from(path))
}

#[derive(Debug, Clone, PartialEq)]
struct ResolvedRenv<'a> {
    package_info: &'a PackageInfo,
    source: Source<'a>,
}

#[derive(Debug, Clone, PartialEq)]
enum Source<'a> {
    Repository(&'a RenvRepository),
    GitHub{git: String, sha: String},
    Local(PathBuf)
}

struct UnresolvedRenv<'a> {
    package_info: &'a PackageInfo,
    error: Box<dyn Error>,
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

#[cfg(test)]
mod tests {
    use super::RenvLock;

    #[test]
    fn test_renv_lock_parse() {
        let _renv_lock = RenvLock::parse_renv_lock("src/tests/renv/renv.lock").unwrap();
    }
}
