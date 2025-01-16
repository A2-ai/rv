use crate::cli::DiskCache;
use crate::{version::Version, Repository};
use anyhow::{bail, Result};
use serde::Deserialize;
use std::hash::Hash;
use std::{collections::HashMap, path::Path, str::FromStr};

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
    version: Version,
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
        let rl_file = path.as_ref().join("renv.lock");

        let content = match std::fs::read_to_string(&rl_file) {
            Ok(c) => c,
            Err(e) => {
                return Err(FromFileError {
                    path: rl_file.into(),
                    source: FromFileErrorKind::Io(e),
                })
            }
        };

        serde_json::from_str(content.as_str()).map_err(|e| FromFileError {
            path: rl_file.into(),
            source: FromFileErrorKind::Parse(e),
        })
    }

    pub fn r_version(&self) -> &Version {
        &self.r.version
    }

    pub fn repositories(&self) -> &Vec<Repository> {
        &self.r.repositories
    }
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
        let _ =
            RenvLock::parse_renv_lock("src/tests/renv/")
                .unwrap();
    }
}
