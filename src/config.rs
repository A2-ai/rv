use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::lockfile::Source;
use crate::package::{deserialize_version, Version};
use serde::Deserialize;
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Author {
    name: String,
    email: String,
    #[serde(default)]
    maintainer: bool,
}

impl Author {
    fn to_toml_table(&self) -> InlineTable {
        let mut table = InlineTable::new();
        table.insert("name", self.name.as_str().into());
        table.insert("email", self.email.as_str().into());
        if self.maintainer {
            table.insert("maintainer", true.into());
        }
        table
    }
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

    fn as_toml_value(&self, repo_hash: &HashMap<&str, &str>) -> Value {
        match self {
            Self::Simple(pkg) => Value::from(pkg.as_str()),
            Self::Git {
                git,
                commit,
                tag,
                branch,
                directory,
                name,
                install_suggestions,
            } => {
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
                directory
                    .as_deref()
                    .map(|d| table.insert("directory", d.into()));
                if *install_suggestions {
                    table.insert("install_suggestions", true.into());
                }
                table.into()
            }
            Self::Local {
                path,
                name,
                install_suggestions,
            } => {
                let mut table = InlineTable::new();
                table.insert("name", name.into());
                table.insert("path", path.to_string_lossy().as_ref().into());
                if *install_suggestions {
                    table.insert("install_suggestions", true.into());
                }
                table.into()
            }
            Self::Detailed {
                name,
                repository,
                install_suggestions,
                force_source,
            } => {
                let mut table = InlineTable::new();
                table.insert("name", name.into());
                if let Some(r) = repository.as_deref() {
                    // repository alias is replaced by url as part of finalize, therefore re-finding the alias should always happen
                    // In edge cases where repository alias is not found, do not specify and let resolver/lockfile determine package source
                    if let Some(name) = repo_hash.get(r) {
                        table.insert("repository", Value::from(*name));
                    }
                }
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

impl Project {
    fn edit_toml_table(&self, table: &mut Table) -> Result<(), ConfigErrorKind> {
        // hashmap of urls to alias to convert back `finalize` augmenting the url into the repositories field of ConfigDependency::Detailed
        let repo_hash = self
            .repositories
            .iter()
            .map(|r| (r.url.as_str(), r.alias.as_str()))
            .collect::<HashMap<_, _>>();

        table.insert("name", self.name.as_str().into());
        table.insert("r_version", self.r_version.original.as_str().into());

        if !self.description.is_empty() {
            table.insert("description", self.description.as_str().into());
        }
        self.license
            .as_ref()
            .map(|l| table.insert("license", l.as_str().into()));
        if !self.authors.is_empty() {
            let mut authors = Vec::new();
            for a in &self.authors {
                authors.push(Value::from(a.to_toml_table()));
            }
            let authors = format_array(authors);
            table.insert("authors", authors.into());
        }
        if !self.keywords.is_empty() {
            let mut keywords = Array::new();
            for k in &self.keywords {
                keywords.push(Value::from(k));
            }
            table.insert("keywords", keywords.into());
        }
        // Repositories field is mandatory. If in init or renv migration the repositories is not set, want to create blank field
        let mut repos = Vec::new();
        for r in &self.repositories {
            repos.push(Value::from(r.to_toml_table()));
        }
        let repos = format_array(repos);
        table.insert("repositories", Item::Value(Value::Array(repos)));

        if !self.suggests.is_empty() {
            let mut suggests = Vec::new();
            for s in &self.suggests {
                suggests.push(s.as_toml_value(&repo_hash));
            }
            let suggests = format_array(suggests);
            table.insert("suggests", suggests.into());
        }

        if !self.urls.is_empty() {
            let url_table = table
                .entry("urls")
                .or_insert(Item::Table(Table::new()))
                .as_table_mut()
                .ok_or_else(|| {
                    ConfigErrorKind::InvalidConfig(
                        "`[project.urls]` exists but is not a table".to_string(),
                    )
                })?;
            for (k, v) in &self.urls {
                url_table.insert(&k, v.as_str().into());
            }
        }

        // Dependencies field is mandatory. Want to have blank field if no dependencies in init
        let mut deps = Vec::new();
        for d in &self.dependencies {
            deps.push(d.as_toml_value(&repo_hash));
        }
        let deps = format_array(deps);
        table.insert("dependencies", deps.into());

        if !self.dev_dependencies.is_empty() {
            let mut dev_deps = Vec::new();
            for d in &self.dev_dependencies {
                dev_deps.push(d.as_toml_value(&repo_hash));
            }
            let dev_deps = format_array(dev_deps);
            table.insert("dev_dependencies", dev_deps.into());
        }

        Ok(())
    }
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
    /// 3. replace the alias in the dependency by the URLx
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

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), ConfigError> {
        let file_path = path.as_ref();
        let content = if file_path.exists() {
            fs::read_to_string(file_path).map_err(|e| ConfigError {
                path: file_path.into(),
                source: ConfigErrorKind::Io(e),
            })?
        } else {
            String::new()
        };

        let mut doc = if content.is_empty() {
            DocumentMut::new()
        } else {
            content.parse::<DocumentMut>().map_err(|e| ConfigError {
                path: file_path.into(),
                source: ConfigErrorKind::DocParse(e),
            })?
        };

        let project = doc
            .entry("project")
            .or_insert(Item::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| ConfigError {
                path: file_path.into(),
                source: ConfigErrorKind::InvalidConfig(
                    "Could not parse Project as table".to_string(),
                ),
            })?;

        self.project
            .edit_toml_table(project)
            .map_err(|ek| ConfigError {
                path: file_path.into(),
                source: ek,
            })?;

        fs::write(file_path, doc.to_string()).map_err(|e| ConfigError {
            path: file_path.into(),
            source: ConfigErrorKind::Io(e),
        })
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

fn format_array(vals: Vec<Value>) -> Array {
    let mut array = vals
        .into_iter()
        .map(|mut v| {
            v.decor_mut().set_prefix("\n    ");
            v
        })
        .collect::<Array>();
    array.set_trailing_comma(true);
    array.set_trailing("\n");
    // if empty, want blank line to prompt user to add value
    // should only reach this function as empty for repositories and dependencies fields
    if array.is_empty() {
        array.set_trailing("\n\n");
    }
    array
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
    DocParse(#[from] toml_edit::TomlError),
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn can_parse_valid_config_files() {
        let paths = std::fs::read_dir("src/tests/config/valid_config/").unwrap();
        for path in paths {
            let res = Config::from_file(path.unwrap().path());
            println!("{res:?}");
            assert!(res.is_ok());
        }
    }

    // TODO: convert to snapshot test
    #[test]
    fn can_edit_config() {
        let config = Config {
            project: Project {
                name: "test".to_string(),
                r_version: Version::from_str("4.4.1").unwrap(),
                description: String::new(),
                license: None,
                authors: Vec::new(),
                keywords: Vec::new(),
                repositories: vec![Repository::new(
                    "a2-ai".to_string(),
                    "some/url".to_string(),
                    true,
                )],
                suggests: Vec::new(),
                urls: HashMap::new(),
                dependencies: Vec::new(),
                dev_dependencies: Vec::new(),
            },
        };

        config.save("src/tests/config/valid_config/all_fields.toml").unwrap();
    }
}
