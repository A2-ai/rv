use std::{collections::HashMap, path::Path};

use serde::Deserialize;

use crate::package::{deserialize_version, Version};

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
