use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

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
pub(crate) struct Repository {
    alias: String,
    url: String,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Dependency {
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

impl Dependency {
    pub fn name(&self) -> &str {
        match self {
            Dependency::Simple(s) => s,
            Dependency::Detailed { name, .. } => name,
        }
    }

    pub fn repository(&self) -> Option<&str> {
        match self {
            Dependency::Simple(_) => None,
            Dependency::Detailed { repository, .. } => repository.as_deref(),
        }
    }

    pub fn force_source(&self) -> bool {
        match self {
            Dependency::Simple(_) => false,
            Dependency::Detailed { force_source, .. } => *force_source,
        }
    }

    pub fn install_suggestions(&self) -> bool {
        match self {
            Dependency::Simple(_) => false,
            Dependency::Detailed {
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
    version: Option<String>,
    license: Option<String>,
    #[serde(default)]
    authors: Vec<Author>,
    #[serde(default)]
    keywords: Vec<String>,
    repositories: Vec<Repository>,
    #[serde(default)]
    suggests: Vec<Dependency>,
    pub r_version: Option<String>,
    #[serde(default)]
    urls: HashMap<String, String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Config {
    pub project: Project,
}

impl Config {
    // TODO: handle errors later
    pub fn from_file<P: AsRef<Path>>(path: P) -> Self {
        // TODO: use a custom read file method that reports the filepath if it fails
        let content = std::fs::read_to_string(path).expect("TODO: handle error");
        toml::from_str(content.as_str()).expect("TODO: handle error")
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
