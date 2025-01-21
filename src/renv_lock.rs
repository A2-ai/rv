use std::{collections::HashMap, path::Path, str::FromStr};

use serde::Deserialize;

use crate::{Repository, Version};

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
    Other(String)
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct PackageInfo {
    pub(crate) package: String,
    #[serde(deserialize_with = "deserialize_version")]
    pub(crate) version: Version,
    pub(crate) source: RenvSource,
    #[serde(default)]
    repository: Option<String>, // when source is Repository
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
    hash: String
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RInfo {
    #[serde(deserialize_with = "deserialize_version")]
    version: Version,
    repositories: Vec<Repository>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct RenvLock {
    r: RInfo,
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

    pub(crate) fn repositories(&self) -> &Vec<Repository> {
        &self.r.repositories
    }

    pub(crate) fn r_version(&self) -> &Version {
        &self.r.version
    }
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
    use super::RenvLock;

    #[test]
    fn test_renv_lock_parse() {
        RenvLock::parse_renv_lock("src/tests/renv/renv.lock").unwrap();
    }
}