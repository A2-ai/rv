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
struct Repository {
    alias: String,
    url: String,
}

// TODO: use enum for dependencies? do it when the config schema is more defined probably
#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Dependency {
    repository: Option<String>,
    url: Option<String>,
    #[serde(default)]
    install_suggestions: bool,
    #[serde(default)]
    force_source: bool,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Project {
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
    r_version: Option<String>,
    #[serde(default)]
    urls: HashMap<String, String>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct Config {
    project: Project,
    #[serde(default)]
    dependencies: HashMap<String, Dependency>,
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
            let res = Config::from_file(path.unwrap().path());
        }
    }
}
