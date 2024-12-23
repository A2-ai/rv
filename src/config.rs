use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::r_cmd::RCmd;
use crate::version::Version;

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
    pub url: String,
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
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Project {
    name: String,
    #[serde(default)]
    description: String,
    version: Option<Version>,
    license: Option<String>,
    #[serde(default)]
    authors: Vec<Author>,
    #[serde(default)]
    keywords: Vec<String>,
    repositories: Vec<Repository>,
    #[serde(default)]
    suggests: Vec<DependencyKind>,
    r_version: Option<Version>,
    #[serde(default)]
    urls: HashMap<String, String>,
    #[serde(default)]
    dependencies: Vec<DependencyKind>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    pub(crate) project: Project,
}

impl Config {
    // TODO: handle errors later
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        // TODO: use a custom read file method that reports the filepath if it fails
        let content = std::fs::read_to_string(path).expect("TODO: handle error");
        toml::from_str(content.as_str()).expect("TODO: handle error")
    }

    /// Gets the R version we want to use in this project.
    /// This will default to whatever is set in the config if set, otherwise pick the output
    /// from the trait call
    pub fn get_r_version(&self, r_cli: impl RCmd) -> Version {
        if let Some(v) = &self.project.r_version {
            v.clone()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_valid_config_files() {
        let paths = std::fs::read_dir("src/tests/valid_config/").unwrap();
        for path in paths {
            // TODO: later it will return a res.
            let _ = Config::from_file(path.unwrap().path());
        }
    }
}
