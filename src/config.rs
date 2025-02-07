use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::lockfile::Source;
use crate::package::{deserialize_version, Version};
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
pub struct Repository {
    pub alias: String,
    url: String,
    #[serde(default)]
    pub force_source: bool,
}

impl fmt::Display for Repository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.force_source {
            write!(
                f,
                r#"{{alias = "{}", url = "{}", force_source = "{}"}}"#,
                self.alias,
                self.url(),
                self.force_source
            )
        } else {
            write!(f, r#"{{alias = "{}", url = "{}"}}"#, self.alias, self.url())
        }
    }
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
#[serde(deny_unknown_fields)]
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
    /// By default, we will always follow the remotes defined in a DESCRIPTION file
    /// It is possible to override this behaviour by setting the package name in that vector if
    /// the following conditions are met:
    /// 1. the package has a version requirement
    /// 2. we can find a package matching that version requirement in a repository
    ///
    /// If a package doesn't list a version requirement in the DESCRIPTION file, we will ALWAYS
    /// install from the remote.
    #[serde(default)]
    prefer_repositories_for: Vec<String>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub(crate) project: Project,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigLoadError> {
        let content = match std::fs::read_to_string(path.as_ref()) {
            Ok(c) => c,
            Err(e) => {
                return Err(ConfigLoadError {
                    path: path.as_ref().into(),
                    source: ConfigLoadErrorKind::Io(e),
                })
            }
        };
        Self::from_str(&content)
    }

    /// This will do 2 things:
    /// 1. verify alias used in deps are found
    /// 2. verify git sources are valid (eg no tag and branch at the same time)
    /// 3. replace the alias in the dependency by the URL
    pub(crate) fn finalize(&mut self) -> Result<(), ConfigLoadError> {
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
            return Err(ConfigLoadError {
                path: Path::new(".").into(),
                source: ConfigLoadErrorKind::InvalidConfig(errors.join("\n")),
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

    pub fn prefer_repositories_for(&self) -> &[String] {
        &self.project.prefer_repositories_for
    }

    pub fn r_version(&self) -> &Version {
        &self.project.r_version
    }
}

impl FromStr for Config {
    type Err = ConfigLoadError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut config: Self = toml::from_str(s).map_err(|e| ConfigLoadError {
            path: Path::new(".").into(),
            source: ConfigLoadErrorKind::Parse(e),
        })?;
        config.finalize()?;
        Ok(config)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to load config at `{path}`")]
#[non_exhaustive]
pub struct ConfigLoadError {
    pub path: Box<Path>,
    pub source: ConfigLoadErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum ConfigLoadErrorKind {
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
}
