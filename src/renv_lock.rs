use std::hash::Hash;
use std::{collections::HashMap, path::Path, str::FromStr};

use anyhow::Result;
use serde::Deserialize;

use crate::{package::Package, version::Version, Repository, RepositoryDatabase};

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
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub(crate) struct RInfo {
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_version")]
    #[serde(rename = "Version")]
    pub(crate) version: Version,
    #[serde(rename = "Repositories")]
    pub(crate) repositories: Vec<Repository>,
}

#[derive(Debug, PartialEq, Clone, Deserialize, Hash, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) struct PackageInfo {
    #[serde(rename = "Package")]
    pub(crate) package: String,
    #[serde(default, rename = "Version")]
    #[serde(deserialize_with = "deserialize_version")]
    pub(crate) version: Version,
    #[serde(rename = "Source")]
    pub(crate) source: RenvSource,
    #[serde(default, rename = "Repository")]
    pub repository: Option<String>,
    #[serde(default, rename = "RemoteType")]
    pub remote_type: Option<String>,
    #[serde(default, rename = "RemoteHost")]
    pub remote_host: Option<String>,
    #[serde(default, rename = "RemoteRepo")]
    pub remote_repo: Option<String>,
    #[serde(default, rename = "RemoteUsername")]
    pub remote_username: Option<String>,
    #[serde(default, rename = "RemoteSha")]
    pub remote_sha: Option<String>,
    #[serde(default, rename = "RemoteUrl")]
    pub remote_url: Option<String>,
    #[serde(default, rename = "Requirements")]
    pub(crate) requirements: Vec<String>,
    #[serde(rename = "Hash")]
    hash: String,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Hash, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum RenvSource {
    Repository,
    GitHub,
    Local,
    Other(String),
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "lowercase")]
pub struct RenvLock {
    #[serde(rename = "R")]
    pub(crate) r: RInfo,
    #[serde(rename = "Packages")]
    pub(crate) packages: HashMap<String, PackageInfo>,
}

impl RenvLock {
    pub fn parse_renv_lock<P: AsRef<Path>>(path: P) -> Result<Self, FromFileError> {
        let path = path.as_ref();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                return Err(FromFileError {
                    path: path.into(),
                    source: FromFileErrorKind::Io(e),
                })
            }
        };

        serde_json::from_str(content.as_str()).map_err(|e| FromFileError {
            path: path.into(),
            source: FromFileErrorKind::Parse(e),
        })
    }

    pub fn r_version(&self) -> &Version {
        &self.r.version
    }

    pub fn repositories(&self) -> &Vec<Repository> {
        &self.r.repositories
    }

    pub fn resolve(&self, databases: Vec<(RepositoryDatabase, bool)>) -> (Vec<ResolvedLock>, Vec<UnresolvedLock>) {
        let mut resolved_lock = Vec::new();
        let mut unresolved_lock = Vec::new();
        for (pkg_name, pkg_info) in &self.packages {
            let resolved = match pkg_info.source {
                RenvSource::Repository => ResolvedLock::resolve_repository(&pkg_name, pkg_info, &databases, self.r_version(), self.repositories()),
                RenvSource::GitHub => ResolvedLock::resolve_github(pkg_info),
                RenvSource::Local => pkg_info.remote_url.as_ref().map(|path| 
                    ResolvedLock {
                        package: Package::from_package_info(&pkg_info),
                        source: PackageSource::Local(path.clone()),
                    }),
                _ => None,
            };
            if let Some(r) = resolved {
                resolved_lock.push(r);
            } else {
                unresolved_lock.push(UnresolvedLock{package: pkg_info.clone()});
            }
        };
        (resolved_lock, unresolved_lock)
    }

    fn resolve_repository(
        pkg_name: &String,
        pkg_info: PackageInfo,
        databases: Vec<(RepositoryDatabase, bool)>,
        r_version: &Version,
        repos: &Vec<Repository>,
    ) -> Option<ResolvedLock> {
        // search for the package in all RepositoryDatabases and create a HashMap keyed on the repo name
        let mut pkgs = Vec::new();
        for (repo_db, force_source) in &databases {
            if let Some((pkg, _)) =
                repo_db.find_package(&pkg_name, None, r_version, *force_source)
            {
                pkgs.push((&repo_db.name, pkg.clone()));
            }
        }

        let pkg_info_repo = &pkg_info.repository?;

        // check if we found an entry in the repository database specified by the package
        if let Some((_, pkg)) = pkgs
            .iter()
            .find(|(repo_name, _)| repo_name == &pkg_info_repo)
        {
            if let Some(repo) = repos
                .iter()
                .find(|r| &r.alias == pkg_info_repo)
            {
                log::debug!("{} resolved successfully", pkg.name);
                return Some(ResolvedLock {
                    source: PackageSource::Repo(repo.clone()),
                    package: pkg.clone(),
                })
            }
        }

        // take the first package found if not
        if let Some((repo_name, pkg)) = pkgs.first() {
            if let Some(repo) = repos.iter().find(|r| &&r.alias == repo_name) {
                log::debug!(
                    "{} not found in specified repository, but found elsewhere",
                    pkg.name
                );

                return Some(ResolvedLock {
                    source: PackageSource::Repo(repo.clone()),
                    package: pkg.clone(),
                })
            }
        }

        // if no entry we can't resolve
        log::warn!(
            "{} not resolved. Manual adjustment needed",
            pkg_info.package
        );
        None
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PackageSource {
    Repo(Repository),
    Git(GitHubSource),
    Local(String),
    Other(String),
}

#[derive(Debug, Clone)]
pub(crate) struct GitHubSource {
    url: String,
    sha: String
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedLock {
    pub(crate) source: PackageSource,
    pub(crate) package: Package,
}

impl ResolvedLock {
    fn resolve_repository(
        pkg_name: &String,
        pkg_info: &PackageInfo,
        databases: &Vec<(RepositoryDatabase, bool)>,
        r_version: &Version,
        repos: &Vec<Repository>,
    ) -> Option<Self> {
        // search for the package in all RepositoryDatabases and create a HashMap keyed on the repo name
        let mut pkgs = Vec::new();
        for (repo_db, force_source) in databases {
            if let Some((pkg, _)) =
                repo_db.find_package(&pkg_name, None, r_version, *force_source)
            {
                pkgs.push((&repo_db.name, pkg.clone()));
            }
        }

        let pkg_info_repo = &pkg_info.repository.clone()?;

        // check if we found an entry in the repository database specified by the package
        if let Some((_, pkg)) = pkgs
            .iter()
            .find(|(repo_name, _)| repo_name == &pkg_info_repo)
        {
            if let Some(repo) = repos
                .iter()
                .find(|r| &r.alias == pkg_info_repo)
            {
                log::debug!("{} resolved successfully", pkg.name);
                return Some(ResolvedLock {
                    source: PackageSource::Repo(repo.clone()),
                    package: pkg.clone(),
                })
            }
        }

        // take the first package found if not
        if let Some((repo_name, pkg)) = pkgs.first() {
            if let Some(repo) = repos.iter().find(|r| &&r.alias == repo_name) {
                log::debug!(
                    "{} not found in specified repository, but found elsewhere",
                    pkg.name
                );

                return Some(ResolvedLock {
                    source: PackageSource::Repo(repo.clone()),
                    package: pkg.clone(),
                })
            }
        }

        // if no entry we can't resolve
        log::warn!(
            "{} not resolved. Manual adjustment needed",
            pkg_info.package
        );
        None
    }

    fn resolve_github(pkg_info: &PackageInfo) -> Option<Self> {
        let package = Package::from_package_info(&pkg_info);
        let no_api = pkg_info.remote_host.clone()?.replace("api.", "");
        let remote = &no_api.trim_end_matches("/api/v3");
        let git_url = format!("https://{}/{}/{}", remote, &pkg_info.remote_username.clone()?, &pkg_info.remote_repo.clone()?);
        Some(Self {
            package,
            source: PackageSource::Git(GitHubSource { url: git_url, sha: pkg_info.remote_sha.clone()?})
        })
    }
}

#[derive(Debug, Clone)]
pub struct UnresolvedLock {
    package: PackageInfo,
}

#[derive(Debug, thiserror::Error)]
#[error("Error reading `{path}`")]
#[non_exhaustive]
pub struct FromFileError {
    pub path: Box<Path>,
    pub source: FromFileErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum FromFileErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] serde_json::Error),
}

mod tests {
    use super::*;
    use crate::{
        cli::{context::load_databases, DiskCache},
        SystemInfo
    };

    #[test]
    fn test_parse_renv_lock() {
        let tmp = RenvLock::parse_renv_lock("src/tests/renv2/renv.lock").unwrap();
        println!("{:#?}", tmp);
    }

    #[test]
    fn test_resolve_renv() {
        let start = std::time::Instant::now();
        let renv_lock = RenvLock::parse_renv_lock("src/tests/renv2/renv.lock").unwrap();
        let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info()).unwrap();
        let databases = load_databases(renv_lock.repositories(), &cache).unwrap();
        let (_, _) = renv_lock.resolve(databases);
        println!("{}", start.elapsed().as_millis());
    }
}
