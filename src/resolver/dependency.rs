use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use url::Url;

use crate::cache::InstallationStatus;
use crate::lockfile::{LockedPackage, Source};
use crate::package::{Dependency, InstallationDependencies, Package, PackageRemote, PackageType};
use crate::resolver::QueueItem;
use crate::{Version, VersionRequirement};

/// A dependency that we found from any of the sources we can look up to
/// We use Cow everywhere because only for git/local packages will be owned, the vast majority
/// will be borrowed
#[derive(PartialEq, Clone)]
pub struct ResolvedDependency<'d> {
    pub name: Cow<'d, str>,
    pub version: Cow<'d, Version>,
    pub source: Source,
    pub(crate) dependencies: Vec<Cow<'d, Dependency>>,
    pub(crate) suggests: Vec<Cow<'d, Dependency>>,
    pub(crate) force_source: bool,
    pub(crate) install_suggests: bool,
    pub(crate) kind: PackageType,
    pub(crate) installation_status: InstallationStatus,
    pub(crate) path: Option<Cow<'d, str>>,
    pub from_lockfile: bool,
    pub(crate) from_remote: bool,
    // Remotes are only for local/git deps so the values will always be owned
    pub(crate) remotes: HashMap<String, (Option<String>, PackageRemote)>,
    // Only set for local dependencies. This is the full resolved path to a directory/tarball
    pub(crate) local_resolved_path: Option<PathBuf>,
    pub(crate) env_vars: HashMap<&'d str, &'d str>,
    /// Whether this dependency should be ignored by the sync handler.
    /// This can happen for example if you have
    /// { name = "dplyr", dependencies_only = true } in your rproject.toml
    /// in which case we want to keep track of it but not write it anywhere
    pub(crate) ignored: bool,
}

impl<'d> ResolvedDependency<'d> {
    pub fn is_installed(&self) -> bool {
        match self.kind {
            PackageType::Source => self.installation_status.source_available(),
            PackageType::Binary => self.installation_status.binary_available(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self.source, Source::Local { .. })
    }

    pub fn all_dependencies_names(&'d self) -> Vec<&'d str> {
        let mut deps: HashSet<_> = self.dependencies.iter().map(|x| x.name()).collect();
        if self.install_suggests {
            for s in &self.suggests {
                deps.insert(s.name());
            }
        }

        deps.into_iter().collect()
    }

    /// We found the dependency from the lockfile
    pub fn from_locked_package(
        package: &'d LockedPackage,
        installation_status: InstallationStatus,
    ) -> Self {
        Self {
            name: Cow::Borrowed(&package.name),
            version: Cow::Owned(Version::from_str(package.version.as_str()).unwrap()),
            source: package.source.clone(),
            dependencies: package.dependencies.iter().map(Cow::Borrowed).collect(),
            suggests: package.suggests.iter().map(Cow::Borrowed).collect(),
            // TODO: what should we do here?
            kind: if package.force_source {
                PackageType::Source
            } else {
                PackageType::Binary
            },
            force_source: package.force_source,
            install_suggests: package.install_suggests(),
            path: package.path.as_ref().map(|x| Cow::Borrowed(x.as_str())),
            from_lockfile: true,
            installation_status,
            remotes: HashMap::new(),
            // it might come from a remote but we don't keep track of that
            from_remote: false,
            local_resolved_path: None,
            env_vars: HashMap::new(),
            ignored: false,
        }
    }

    pub fn from_package_repository(
        package: &'d Package,
        repo_url: &Url,
        package_type: PackageType,
        install_suggests: bool,
        force_source: bool,
        installation_status: InstallationStatus,
    ) -> (Self, InstallationDependencies<'d>) {
        let deps = package.dependencies_to_install(install_suggests);
        let source = match (&package.remote_url, &package.remote_sha) {
            (Some(git), Some(sha)) if repo_url.to_string().contains("r-universe.dev") => {
                Source::RUniverse {
                    repository: repo_url.clone(),
                    git: git.clone(),
                    sha: sha.to_string(),
                    directory: package.remote_subdir.clone(),
                }
            }
            _ => Source::Repository {
                repository: repo_url.clone(),
            },
        };

        let res = Self {
            name: Cow::Borrowed(&package.name),
            version: Cow::Borrowed(&package.version),
            source,
            dependencies: deps.direct.iter().map(|d| Cow::Borrowed(*d)).collect(),
            suggests: deps.suggests.iter().map(|d| Cow::Borrowed(*d)).collect(),
            kind: package_type,
            force_source,
            install_suggests,
            path: package.path.as_ref().map(|x| Cow::Borrowed(x.as_str())),
            from_lockfile: false,
            installation_status,
            remotes: HashMap::new(),
            from_remote: false,
            local_resolved_path: None,
            env_vars: HashMap::new(),
            ignored: false,
        };

        (res, deps)
    }

    /// If we find the package to be a git repo, we will read the DESCRIPTION file during resolution
    /// This means the data will not outlive this struct and needs to be owned
    pub fn from_git_package(
        package: &Package,
        source: Source,
        install_suggests: bool,
        installation_status: InstallationStatus,
    ) -> (Self, InstallationDependencies) {
        let deps = package.dependencies_to_install(install_suggests);

        let res = Self {
            dependencies: deps.direct.iter().map(|&d| Cow::Owned(d.clone())).collect(),
            suggests: deps
                .suggests
                .iter()
                .map(|&d| Cow::Owned(d.clone()))
                .collect(),
            kind: PackageType::Source,
            force_source: true,
            path: None,
            from_lockfile: false,
            name: Cow::Owned(package.name.clone()),
            version: Cow::Owned(package.version.clone()),
            source,
            installation_status,
            install_suggests,
            remotes: package.remotes.clone(),
            from_remote: false,
            local_resolved_path: None,
            env_vars: HashMap::new(),
            ignored: false,
        };

        (res, deps)
    }

    pub fn from_local_package(
        package: &Package,
        source: Source,
        install_suggests: bool,
        local_resolved_path: PathBuf,
    ) -> (Self, InstallationDependencies) {
        let deps = package.dependencies_to_install(install_suggests);
        let res = Self {
            dependencies: deps.direct.iter().map(|&d| Cow::Owned(d.clone())).collect(),
            suggests: deps
                .suggests
                .iter()
                .map(|&d| Cow::Owned(d.clone()))
                .collect(),
            kind: PackageType::Source,
            force_source: true,
            path: None,
            from_lockfile: false,
            name: Cow::Owned(package.name.clone()),
            version: Cow::Owned(package.version.clone()),
            source,
            // We'll handle the installation status later by comparing mtimes
            installation_status: InstallationStatus::Source,
            install_suggests,
            remotes: package.remotes.clone(),
            from_remote: false,
            local_resolved_path: Some(local_resolved_path),
            env_vars: HashMap::new(),
            ignored: false,
        };

        (res, deps)
    }

    pub fn from_url_package(
        package: &Package,
        kind: PackageType,
        source: Source,
        install_suggests: bool,
    ) -> (Self, InstallationDependencies) {
        let deps = package.dependencies_to_install(install_suggests);
        let res = Self {
            dependencies: deps.direct.iter().map(|&d| Cow::Owned(d.clone())).collect(),
            suggests: deps
                .suggests
                .iter()
                .map(|&d| Cow::Owned(d.clone()))
                .collect(),
            kind,
            force_source: false,
            path: None,
            from_lockfile: false,
            name: Cow::Owned(package.name.clone()),
            version: Cow::Owned(package.version.clone()),
            source,
            installation_status: InstallationStatus::Source,
            install_suggests,
            remotes: package.remotes.clone(),
            from_remote: false,
            local_resolved_path: None,
            env_vars: HashMap::new(),
            ignored: false,
        };

        (res, deps)
    }

    pub fn from_builtin_package(
        package: &'d Package,
        install_suggests: bool,
    ) -> (Self, InstallationDependencies<'d>) {
        let deps = package.dependencies_to_install(install_suggests);

        let res = Self {
            name: Cow::Borrowed(&package.name),
            version: Cow::Borrowed(&package.version),
            source: Source::Builtin { builtin: true },
            dependencies: deps.direct.iter().map(|d| Cow::Borrowed(*d)).collect(),
            suggests: deps.suggests.iter().map(|d| Cow::Borrowed(*d)).collect(),
            kind: PackageType::Binary,
            force_source: false,
            install_suggests,
            path: package.path.as_ref().map(|x| Cow::Borrowed(x.as_str())),
            from_lockfile: false,
            installation_status: InstallationStatus::Binary(false),
            remotes: HashMap::new(),
            from_remote: false,
            local_resolved_path: None,
            env_vars: HashMap::new(),
            ignored: false,
        };

        (res, deps)
    }
}

impl fmt::Debug for ResolvedDependency<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut vars = self
            .env_vars
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>();
        vars.sort();
        write!(
            f,
            "{}={} ({:?}, type={}, path='{}', from_lockfile={}, from_remote={}, env_vars=[{}]{})",
            self.name,
            self.version.original,
            self.source,
            self.kind,
            self.path.as_deref().unwrap_or(""),
            self.from_lockfile,
            self.from_remote,
            vars.join(", "),
            if self.ignored { ", ignored" } else { "" },
        )
    }
}

/// A dependency that we could not
#[derive(Debug, PartialEq, Clone)]
pub struct UnresolvedDependency<'d> {
    pub(crate) name: Cow<'d, str>,
    pub(crate) error: Option<String>,
    pub(crate) version_requirement: Option<Cow<'d, VersionRequirement>>,
    // The first parent we encountered requiring that package
    pub(crate) parent: Option<Cow<'d, str>>,
    pub(crate) remote: Option<PackageRemote>,
    pub(crate) local_path: Option<PathBuf>,
    pub(crate) url: Option<String>,
}

impl<'d> UnresolvedDependency<'d> {
    pub(crate) fn from_item(item: &QueueItem<'d>) -> Self {
        Self {
            name: item.name.clone(),
            error: None,
            version_requirement: item.version_requirement.clone(),
            parent: item.parent.clone(),
            remote: None,
            local_path: item.local_path.clone(),
            url: None,
        }
    }

    pub(crate) fn with_error(mut self, err: String) -> Self {
        self.error = Some(err);
        self
    }

    pub(crate) fn with_remote(mut self, remote: PackageRemote) -> Self {
        self.remote = Some(remote);
        self
    }

    pub(crate) fn with_url(mut self, url: &str) -> Self {
        self.url = Some(url.to_string());
        self
    }

    pub fn is_listed_in_config(&self) -> bool {
        self.parent.is_none()
    }
}

impl fmt::Display for UnresolvedDependency<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{} {}{}",
            self.name,
            if let Some(l) = &self.version_requirement {
                format!(" {l} ")
            } else {
                String::new()
            },
            if self.is_listed_in_config() {
                "[listed in rproject.toml]".to_string()
            } else {
                format!("[required by: {}]", self.parent.as_ref().unwrap())
            },
            if let Some(e) = &self.error {
                format!(": {}", e)
            } else {
                String::new()
            }
        )
    }
}
