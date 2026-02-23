use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::SystemInfo;
use crate::consts::LOCKFILE_NAME;
use crate::git::url::GitUrl;
use crate::lockfile::Source;
use crate::package::{Version, deserialize_version, serialize_version};
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HttpUrl(Url);

impl<'de> Deserialize<'de> for HttpUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if (s.starts_with("http://") || s.starts_with("https://"))
            && let Ok(mut url) = Url::parse(&s)
        {
            // Remove trailing slashes from the path
            let path = url.path().trim_end_matches('/').to_string();
            url.set_path(&path);
            return Ok(Self(url));
        }

        Err(serde::de::Error::custom("Invalid URL"))
    }
}

impl Deref for HttpUrl {
    type Target = Url;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Author {
    name: String,
    email: String,
    #[serde(default)]
    maintainer: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub alias: String,
    pub(crate) url: HttpUrl,
    #[serde(default)]
    pub force_source: bool,
}

impl Repository {
    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    pub fn new(alias: String, url: Url, force_source: bool) -> Self {
        Self {
            alias,
            url: HttpUrl(url),
            force_source,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
pub enum ConfigDependency {
    Simple(String),
    Git {
        // It can be http or ssh
        git: GitUrl,
        commit: Option<String>,
        tag: Option<String>,
        branch: Option<String>,
        directory: Option<String>,
        name: String,
        #[serde(default)]
        install_suggestions: bool,
        #[serde(default)]
        dependencies_only: bool,
    },
    Local {
        path: PathBuf,
        name: String,
        #[serde(default)]
        install_suggestions: bool,
        #[serde(default)]
        dependencies_only: bool,
    },
    Url {
        url: HttpUrl,
        name: String,
        #[serde(default)]
        install_suggestions: bool,
        #[serde(default)]
        dependencies_only: bool,
    },
    Detailed {
        name: String,
        repository: Option<String>,
        #[serde(default)]
        install_suggestions: bool,
        #[serde(default)]
        force_source: Option<bool>,
        #[serde(default)]
        dependencies_only: bool,
    },
}

impl ConfigDependency {
    pub fn name(&self) -> &str {
        match self {
            ConfigDependency::Simple(s) => s,
            ConfigDependency::Detailed { name, .. } => name,
            ConfigDependency::Git { name, .. } => name,
            ConfigDependency::Local { name, .. } => name,
            ConfigDependency::Url { name, .. } => name,
        }
    }

    pub fn force_source(&self) -> Option<bool> {
        match self {
            ConfigDependency::Detailed { force_source, .. } => *force_source,
            _ => None,
        }
    }

    pub fn r_repository(&self) -> Option<&str> {
        match self {
            ConfigDependency::Detailed { repository, .. } => repository.as_deref(),
            _ => None,
        }
    }

    pub fn local_path(&self) -> Option<PathBuf> {
        match self {
            ConfigDependency::Local { path, .. } => Some(path.clone()),
            _ => None,
        }
    }

    pub fn dependencies_only(&self) -> bool {
        match self {
            ConfigDependency::Git {
                dependencies_only, ..
            } => *dependencies_only,
            ConfigDependency::Local {
                dependencies_only, ..
            } => *dependencies_only,
            ConfigDependency::Url {
                dependencies_only, ..
            } => *dependencies_only,
            ConfigDependency::Detailed {
                dependencies_only, ..
            } => *dependencies_only,
            ConfigDependency::Simple(_) => false,
        }
    }

    pub(crate) fn as_git_source_with_sha(&self, sha: String) -> Source {
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
            }
            | ConfigDependency::Url {
                install_suggestions,
                ..
            }
            | ConfigDependency::Local {
                install_suggestions,
                ..
            }
            | ConfigDependency::Git {
                install_suggestions,
                ..
            } => *install_suggestions,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OsTarget {
    Linux,
    Macos,
    Windows,
}

impl OsTarget {
    fn matches(&self, system_info: &SystemInfo) -> bool {
        use crate::system_info::OsType;
        matches!(
            (&system_info.os_type, self),
            (OsType::Linux(_), OsTarget::Linux)
                | (OsType::MacOs, OsTarget::Macos)
                | (OsType::Windows, OsTarget::Windows)
        )
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchTarget {
    #[serde(alias = "amd64")]
    X86_64,
    #[serde(alias = "aarch64")]
    Arm64,
    X86,
    Arm,
}

impl ArchTarget {
    fn matches(&self, system_info: &SystemInfo) -> bool {
        let current_arch = system_info.arch().unwrap_or("unknown");
        matches!(
            (current_arch, self),
            ("x86_64" | "amd64", ArchTarget::X86_64)
                | ("aarch64" | "arm64", ArchTarget::Arm64)
                | ("x86" | "i386" | "i686", ArchTarget::X86)
                | ("arm" | "armv7", ArchTarget::Arm)
        )
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ConfigureArgsRule {
    OsArch {
        os: OsTarget,
        arch: ArchTarget,
        args: Vec<String>,
    },
    Os {
        os: OsTarget,
        args: Vec<String>,
    },
    Arch {
        arch: ArchTarget,
        args: Vec<String>,
    },
    Default {
        args: Vec<String>,
    },
}

impl ConfigureArgsRule {
    pub fn matches(&self, system_info: &SystemInfo) -> Option<&[String]> {
        match self {
            ConfigureArgsRule::OsArch { os, arch, args } => {
                if os.matches(system_info) && arch.matches(system_info) {
                    Some(args)
                } else {
                    None
                }
            }
            ConfigureArgsRule::Os { os, args } => {
                if os.matches(system_info) {
                    Some(args)
                } else {
                    None
                }
            }
            ConfigureArgsRule::Arch { arch, args } => {
                if arch.matches(system_info) {
                    Some(args)
                } else {
                    None
                }
            }
            ConfigureArgsRule::Default { args } => Some(args),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Project {
    name: String,
    #[serde(
        deserialize_with = "deserialize_version",
        serialize_with = "serialize_version"
    )]
    r_version: Version,
    #[serde(default)]
    use_devel: Option<bool>,
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
    urls: HashMap<String, Url>,
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
    /// This is where you add specific environment variables for each package compilation step,
    /// they will be passed to R.
    /// If a package is already available as binary and you don't mention you want to force source,
    /// this will not be used
    #[serde(default)]
    packages_env_vars: HashMap<String, HashMap<String, String>>,
    /// Package-specific configure.args with system targeting
    #[serde(default)]
    pub configure_args: HashMap<String, Vec<ConfigureArgsRule>>,
    /// Packages for which stripping should be disabled during installation.
    /// By default, rv passes --strip and --strip-lib to R CMD INSTALL.
    /// Packages listed here will be installed without those flags.
    #[serde(default)]
    no_strip: Vec<String>,
}

// That's the way to do it with serde :/
// https://github.com/serde-rs/serde/issues/368
fn default_true() -> bool {
    true
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub(crate) library: Option<PathBuf>,
    #[serde(default = "default_true")]
    pub(crate) use_lockfile: bool,
    lockfile_name: Option<String>,
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
                });
            }
        };
        let mut config: Self = toml::from_str(&content).map_err(|e| ConfigLoadError {
            path: path.as_ref().into(),
            source: ConfigLoadErrorKind::Parse(e),
        })?;
        config.finalize(path.as_ref())?;
        Ok(config)
    }

    /// This will do 2 things:
    /// 1. verify alias used in deps are found
    /// 2. verify git sources are valid (eg no tag and branch at the same time)
    /// 3. replace the alias in the dependency by the URL
    pub(crate) fn finalize(&mut self, path: &Path) -> Result<(), ConfigLoadError> {
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
                            replacement = Some(repo.url().to_string());
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
                } => match (tag.is_some(), branch.is_some(), commit.is_some()) {
                    (true, false, false) | (false, true, false) | (false, false, true) => (),
                    _ => {
                        errors.push(format!("A git dependency `{git}` requires ons and only one of tag/branch/commit set. "));
                    }
                },
                _ => (),
            }
        }

        if !errors.is_empty() {
            return Err(ConfigLoadError {
                path: path.into(),
                source: ConfigLoadErrorKind::InvalidConfig(errors.join("\n")),
            });
        }

        Ok(())
    }

    pub fn repositories(&self) -> &[Repository] {
        &self.project.repositories
    }

    pub fn repositories_mut(&mut self) -> &mut [Repository] {
        &mut self.project.repositories
    }

    pub fn dependencies(&self) -> &[ConfigDependency] {
        &self.project.dependencies
    }

    pub fn dependencies_mut(&mut self) -> &mut [ConfigDependency] {
        &mut self.project.dependencies
    }

    pub fn prefer_repositories_for(&self) -> &[String] {
        &self.project.prefer_repositories_for
    }

    pub fn packages_env_vars(&self) -> &HashMap<String, HashMap<String, String>> {
        &self.project.packages_env_vars
    }

    pub fn r_version(&self) -> &Version {
        &self.project.r_version
    }

    pub fn use_devel(&self) -> bool {
        self.project.use_devel.unwrap_or(false)
    }

    pub fn use_lockfile(&self) -> bool {
        self.use_lockfile
    }

    pub fn library(&self) -> Option<&PathBuf> {
        self.library.as_ref()
    }

    pub fn set_library(&mut self, library: PathBuf) {
        self.library = Some(library);
    }

    pub fn lockfile_name(&self) -> &str {
        self.lockfile_name.as_deref().unwrap_or(LOCKFILE_NAME)
    }

    pub fn get_configure_args(&self, package_name: &str, system_info: &SystemInfo) -> &[String] {
        if let Some(rules) = self.project.configure_args.get(package_name) {
            // Find first matching rule
            for rule in rules {
                if let Some(args) = rule.matches(system_info) {
                    return args;
                }
            }
        }

        &[]
    }

    pub fn configure_args(&self) -> &HashMap<String, Vec<ConfigureArgsRule>> {
        &self.project.configure_args
    }

    pub fn no_strip(&self) -> &[String] {
        &self.project.no_strip
    }
}

impl FromStr for Config {
    type Err = ConfigLoadError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut config: Self = toml::from_str(s).map_err(|e| ConfigLoadError {
            path: Path::new(".").into(),
            source: ConfigLoadErrorKind::Parse(e),
        })?;
        config.finalize(Path::new("."))?;
        Ok(config)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to load config at `{path}`\n\nCaused by:\n  {source}")]
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

    #[test]
    fn errors_on_invalid_config_files() {
        let paths = std::fs::read_dir("src/tests/invalid_config/").unwrap();
        for path in paths {
            println!("{path:?}");
            let res = Config::from_file(path.unwrap().path());
            println!("{res:#?}");
            assert!(res.is_err());
        }
    }

    #[test]
    fn can_parse_no_strip() {
        let toml_str = r#"
[project]
name = "test"
r_version = "4.4"
repositories = []
no_strip = ["rgl", "sf"]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.no_strip(), &["rgl", "sf"]);
    }

    #[test]
    fn no_strip_defaults_to_empty() {
        let toml_str = r#"
[project]
name = "test"
r_version = "4.4"
repositories = []
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.no_strip().is_empty());
    }

    #[test]
    fn config_r_version_round_trips_as_string() {
        let toml_str = r#"
[project]
name = "test"
r_version = "4.4"
repositories = []
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(
            config.r_version().original,
            deserialized.r_version().original
        );
    }
}
