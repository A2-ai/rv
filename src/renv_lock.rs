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
#[serde(rename_all = "lowercase")]
pub(crate) struct PackageInfo {
    #[serde(rename = "Package")]
    pub(crate) package: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_version")]
    #[serde(rename = "Version")]
    pub(crate) version: Version,
    #[serde(rename = "Source")]
    pub(crate) source: String,
    #[serde(rename = "Repository")]
    pub(crate) repository: String,
    #[serde(default)]
    #[serde(rename = "Requirements")]
    pub(crate) requirements: Option<Vec<String>>,
    #[serde(rename = "Hash")]
    hash: String,
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

    pub(crate) fn resolve(
        &self,
        databases: Vec<(RepositoryDatabase, bool)>,
    ) -> (Vec<ResolvedLock>, Vec<UnresolvedLock>) {
        let mut found_pkg = Vec::new();
        let mut not_found_pkg = Vec::new();
        // loop through all packages from renv.lock file
        for (pkg_name, pkg_info) in &self.packages {
            // search for the package in all RepositoryDatabases and create a HashMap keyed on the repo name
            let mut pkgs = Vec::new();
            for (repo_db, force_source) in &databases {
                if let Some((pkg, _)) =
                    repo_db.find_package(&pkg_name, None, &self.r_version(), *force_source)
                {
                    pkgs.push((&repo_db.name, pkg.clone()));
                }
            }

            // check if we found an entry in the repository database specified by the package
            if let Some((_, pkg)) = pkgs
                .iter()
                .find(|(repo_name, _)| repo_name == &&pkg_info.repository)
            {
                if let Some(repo) = self
                    .repositories()
                    .iter()
                    .find(|r| r.alias == pkg_info.repository)
                {
                    log::debug!("{} resolved successfully", pkg.name);
                    found_pkg.push(ResolvedLock {
                        repository: repo.clone(),
                        package: pkg.clone(),
                    });
                    continue;
                }
            }

            // take the first package found if not
            if let Some((repo_name, pkg)) = pkgs.first() {
                if let Some(repo) = self.repositories().iter().find(|r| &&r.alias == repo_name) {
                    log::debug!(
                        "{} not found in specified repository, but found elsewhere",
                        pkg.name
                    );
                    found_pkg.push(ResolvedLock {
                        repository: repo.clone(),
                        package: pkg.clone(),
                    });
                }
            }

            // if no entry we can't resolve
            log::warn!(
                "{} not resolved. Manual adjustment needed",
                pkg_info.package
            );
            not_found_pkg.push(UnresolvedLock {
                package: pkg_info.clone(),
            });
        }
        (found_pkg, not_found_pkg)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedLock {
    pub(crate) repository: Repository,
    pub(crate) package: Package,
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

    #[test]
    fn test_parse_renv_lock() {
        let _ = RenvLock::parse_renv_lock("src/tests/renv/renv.lock").unwrap();
    }
}
