use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::lockfile::Source;
use crate::package::{deserialize_version, Version};
use serde::Deserialize;
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

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

    fn to_toml_table(&self) -> InlineTable {
        let mut table = InlineTable::new();
        table.insert("alias", self.alias.as_str().into());
        table.insert("url", self.url().into());
        if self.force_source {
            table.insert("force_source", true.into());
        }
        table
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(untagged)]
pub enum ConfigDependency {
    Simple(String),
    Git {
        git: String,
        // TODO: validate that either commit, branch or tag is set
        commit: Option<String>,
        tag: Option<String>,
        branch: Option<String>,
        directory: Option<String>,
        name: String,
        #[serde(default)]
        install_suggestions: bool,
    },
    Local {
        path: PathBuf,
        name: String,
        #[serde(default)]
        install_suggestions: bool,
    },
    Detailed {
        name: String,
        repository: Option<String>,
        #[serde(default)]
        install_suggestions: bool,
        #[serde(default)]
        force_source: bool,
    },
}

impl ConfigDependency {
    pub fn name(&self) -> &str {
        match self {
            ConfigDependency::Simple(s) => s,
            ConfigDependency::Detailed { name, .. } => name,
            ConfigDependency::Git { name, .. } => name,
            ConfigDependency::Local { name, .. } => name,
        }
    }

    pub fn force_source(&self) -> bool {
        match self {
            ConfigDependency::Detailed { force_source, .. } => *force_source,
            _ => false,
        }
    }

    pub fn r_repository(&self) -> Option<&str> {
        match self {
            ConfigDependency::Detailed { repository, .. } => repository.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn as_git_source_with_sha(&self, sha: String) -> Source {
        // git: String,
        // // TODO: validate that either commit, branch or tag is set
        // commit: Option<String>,
        // tag: Option<String>,
        // branch: Option<String>,
        // directory: Option<String>,
        match self.clone() {
            ConfigDependency::Git {
                git,
                directory,
                tag,
                branch,
                ..
            } => Source::Git {
                git,
                sha,
                directory,
                tag,
                branch,
            },
            _ => unreachable!(),
        }
    }

    pub fn install_suggestions(&self) -> bool {
        match self {
            ConfigDependency::Simple(_) => false,
            ConfigDependency::Detailed {
                install_suggestions,
                ..
            } => *install_suggestions,
            ConfigDependency::Local {
                install_suggestions,
                ..
            } => *install_suggestions,
            ConfigDependency::Git {
                install_suggestions,
                ..
            } => *install_suggestions,
        }
    }

    fn as_toml_value(&self) -> Value {
        match self {
            Self::Simple(pkg) => Value::from(pkg.as_str()),
            Self::Git { git, commit, tag, branch, directory, name, install_suggestions } => {
                let mut table = InlineTable::new();
                table.insert("name", name.into());
                table.insert("git", git.into());
                // insert one of commit, tag, branch (in that order). Inserting multiple could lead to conflict
                if let Some(c) = commit {
                    table.insert("commit", c.into());
                } else if let Some(t) = tag {
                    table.insert("tag", t.into());
                } else if let Some(b) = branch {
                    table.insert("branch", b.into());
                }
                directory.as_deref().map(|d| table.insert("directory", d.into()));
                if *install_suggestions {
                    table.insert("install_suggestions", true.into());
                }
                table.into()
            },
            Self::Local { path, name, install_suggestions } => {
                let mut table = InlineTable::new();
                table.insert("name", name.into());
                table.insert("path", path.to_string_lossy().as_ref().into());
                if *install_suggestions {
                    table.insert("install_suggestions", true.into());
                }
                table.into()
            },
            Self::Detailed { name, repository, install_suggestions, force_source } => {
                let mut table = InlineTable::new();
                table.insert("name", name.into());
                repository.as_deref().map(|r| table.insert("repository", r.into()));
                if *install_suggestions {
                    table.insert("install_suggestions", true.into());
                }
                if *force_source {
                    table.insert("force_source", true.into());
                }
                table.into()
            }
        }

    }
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Project {
    name: String,
    #[serde(deserialize_with = "deserialize_version")]
    r_version: Version,
    #[serde(default)]
    description: String,
    license: Option<String>,
    #[serde(default)]
    authors: Vec<Author>,
    #[serde(default)]
    keywords: Vec<String>,
    repositories: Vec<Repository>,
    #[serde(default)]
    suggests: Vec<ConfigDependency>,
    #[serde(default)]
    urls: HashMap<String, String>,
    #[serde(default)]
    dependencies: Vec<ConfigDependency>,
    #[serde(default)]
    dev_dependencies: Vec<ConfigDependency>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub(crate) project: Project,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let content = match std::fs::read_to_string(path.as_ref()) {
            Ok(c) => c,
            Err(e) => {
                return Err(ConfigError {
                    path: path.as_ref().into(),
                    source: ConfigErrorKind::Io(e),
                })
            }
        };
        Self::from_str(&content)
    }

    /// This will do 2 things:
    /// 1. verify alias used in deps are found
    /// 2. verify git sources are valid (eg no tag and branch at the same time)
    /// 3. replace the alias in the dependency by the URL
    pub(crate) fn finalize(&mut self) -> Result<(), ConfigError> {
        let repo_mapping: HashMap<_, _> = self
            .project
            .repositories
            .iter()
            .map(|r| (r.alias.as_str(), r))
            .collect();
        let mut errors = Vec::new();

        for d in self.project.dependencies.iter_mut() {
            match d {
                // If it has a repository set, we need to check the alias is found and replace it with the url
                ConfigDependency::Detailed {
                    repository, name, ..
                } => {
                    if name.trim().is_empty() {
                        errors.push("A dependency is missing a name.".to_string());
                        continue;
                    }

                    let mut replacement = None;
                    if let Some(alias) = repository {
                        if let Some(repo) = repo_mapping.get(alias.as_str()) {
                            replacement = Some(repo.url.clone());
                        } else {
                            errors.push(format!(
                                "Dependency {name} is using alias {alias} which is unknown."
                            ));
                        }
                    }
                    *repository = replacement;
                }
                ConfigDependency::Git {
                    git,
                    tag,
                    branch,
                    commit,
                    ..
                } => {
                    if git.trim().is_empty() {
                        errors.push("A git dependency is missing a URL.".to_string());
                        continue;
                    }
                    match (tag.is_some(), branch.is_some(), commit.is_some()) {
                        (true, false, false) | (false, true, false) | (false, false, true) => (),
                        _ => {
                            errors.push(format!("A git dependency `{git}` requires one and only one of tag/branch/commit set. "));
                        }
                    }
                }
                _ => (),
            }
        }

        if !errors.is_empty() {
            return Err(ConfigError {
                path: Path::new(".").into(),
                source: ConfigErrorKind::InvalidConfig(errors.join("\n")),
            });
        }

        Ok(())
    }

    pub fn repositories(&self) -> &[Repository] {
        &self.project.repositories
    }

    pub fn dependencies(&self) -> &[ConfigDependency] {
        &self.project.dependencies
    }

    pub fn r_version(&self) -> &Version {
        &self.project.r_version
    }

    pub fn save(&self, path: impl AsRef<Path>) {
        let out = self.as_toml_string();
    }

    fn as_toml_string(&self) -> String {
        let mut doc = toml_edit::DocumentMut::new();
        doc.insert("name", Item::Value(Value::from(self.project.name.to_string())));
        doc.insert("r_version", Item::Value(Value::from(self.project.r_version.original.to_string())));
        let mut repos = Array::new();
        for r in self.repositories() {
            repos.push(Value::from(r.to_toml_table()));
        }
        doc.insert("repositories", Item::Value(Value::Array(repos)));

        let mut deps = Array::new();
        for d in self.dependencies() {
            deps.push(Value::from(d.as_toml_value()));
        }
        doc.insert("dependencies", Item::Value(Value::Array(deps)));

        doc.to_string()
    }
}

impl FromStr for Config {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut config: Self = toml::from_str(s).map_err(|e| ConfigError {
            path: Path::new(".").into(),
            source: ConfigErrorKind::Parse(e),
        })?;
        config.finalize()?;
        Ok(config)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to load config at `{path}`")]
#[non_exhaustive]
pub struct ConfigError {
    pub path: Box<Path>,
    pub source: ConfigErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum ConfigErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] toml::de::Error),
    #[error("Invalid config: {0}")]
    InvalidConfig(String),
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

    #[test]
    fn tester() {
        let res = Config::from_file("example_projects/rspm-cran/rproject.toml").unwrap();
        println!("{}", res.as_toml_string());
    }
}
