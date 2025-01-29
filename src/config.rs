use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use crate::r_cmd::{RCmd, VersionError};
use crate::version::Version;
use serde::Deserialize;

fn deserialize_version<'de, D>(deserializer: D) -> Result<Option<Version>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<String> = Deserialize::deserialize(deserializer)?;

    if let Some(v) = v {
        match Version::from_str(&v) {
            Ok(v) => Ok(Some(v)),
            Err(_) => Err(serde::de::Error::custom("Invalid version number")),
        }
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Author {
    name: String,
    email: String,
    #[serde(default)]
    maintainer: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub alias: String,
    url: String,
    #[serde(default)]
    pub force_source: bool,
}

impl Repository {
    /// Returns the URL, always without a trailing URL
    pub fn url(&self) -> &str {
        self.url.trim_end_matches("/")
    }

    pub fn new(alias: String, url: String, force_source: bool) -> Self {
        Self {
            alias,
            url,
            force_source,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub enum DependencyKind {
    Simple(String),
    Detailed {
        name: String,
        repository: Option<String>,
        url: Option<String>,
        #[serde(default)]
        install_suggestions: bool,
        #[serde(default)]
        force_source: bool,
    },
}

impl DependencyKind {
    pub fn name(&self) -> &str {
        match self {
            DependencyKind::Simple(s) => s,
            DependencyKind::Detailed { name, .. } => name,
        }
    }

    pub fn repository(&self) -> Option<&str> {
        match self {
            DependencyKind::Simple(_) => None,
            DependencyKind::Detailed { repository, .. } => repository.as_deref(),
        }
    }

    pub fn force_source(&self) -> bool {
        match self {
            DependencyKind::Simple(_) => false,
            DependencyKind::Detailed { force_source, .. } => *force_source,
        }
    }

    pub fn install_suggestions(&self) -> bool {
        match self {
            DependencyKind::Simple(_) => false,
            DependencyKind::Detailed {
                install_suggestions,
                ..
            } => *install_suggestions,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Project {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_version")]
    version: Option<Version>,
    license: Option<String>,
    #[serde(default)]
    authors: Vec<Author>,
    #[serde(default)]
    keywords: Vec<String>,
    repositories: Vec<Repository>,
    #[serde(default)]
    suggests: Vec<DependencyKind>,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_version")]
    r_version: Option<Version>,
    #[serde(default)]
    urls: HashMap<String, String>,
    #[serde(default)]
    dependencies: Vec<DependencyKind>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub(crate) project: Project,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, FromFileError> {
        let content = match std::fs::read_to_string(path.as_ref()) {
            Ok(c) => c,
            Err(e) => {
                return Err(FromFileError {
                    path: path.as_ref().into(),
                    source: FromFileErrorKind::Io(e),
                })
            }
        };
        Self::from_str(&content)
    }

    /// Gets the R version we want to use in this project.
    /// This will default to whatever is set in the config if set, otherwise pick the output
    /// from the trait call
    pub fn get_r_version(&self, r_cli: impl RCmd) -> Result<Version, VersionError> {
        if let Some(v) = &self.project.r_version {
            Ok(v.clone())
        } else {
            r_cli.version()
        }
    }

    pub fn repositories(&self) -> &[Repository] {
        &self.project.repositories
    }

    pub fn dependencies(&self) -> &[DependencyKind] {
        &self.project.dependencies
    }
}

impl FromStr for Config {
    type Err = FromFileError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        toml::from_str(s).map_err(|e| FromFileError {
            path: Path::new(".").into(),
            source: FromFileErrorKind::Parse(e),
        })
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
    Parse(#[from] toml::de::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_valid_config_files() {
        let paths = std::fs::read_dir("src/tests/valid_config/").unwrap();
        for path in paths {
            let res = Config::from_file(path.unwrap().path());
            println!("{res:?}");
            assert!(res.is_ok());
        }
    }
}
