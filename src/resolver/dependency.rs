use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;

use crate::cache::InstallationStatus;
use crate::lockfile::{LockedPackage, Source};
use crate::package::{InstallationDependencies, Package, PackageRemote, PackageType};
use crate::VersionRequirement;

/// A dependency that we found from any of the sources we can look up to
/// We use Cow everywhere because only for git/local packages will be owned, the vast majority
/// will be borrowed
#[derive(Debug, PartialEq, Clone)]
pub struct ResolvedDependency<'d> {
    pub name: Cow<'d, str>,
    pub(crate) version: Cow<'d, str>,
    pub source: Source,
    pub(crate) dependencies: Vec<Cow<'d, str>>,
    pub(crate) suggests: Vec<Cow<'d, str>>,
    pub(crate) force_source: bool,
    pub(crate) install_suggests: bool,
    pub(crate) kind: PackageType,
    pub(crate) installation_status: InstallationStatus,
    pub(crate) path: Option<Cow<'d, str>>,
    pub(crate) from_lockfile: bool,
    pub(crate) from_remote: bool,
    // Remotes are only for local/git deps so the values will always be owned
    pub(crate) remotes: HashMap<String, (Option<String>, PackageRemote)>,
}

impl<'d> ResolvedDependency<'d> {
    pub fn is_installed(&self) -> bool {
        match self.kind {
            PackageType::Source => self.installation_status.source_available(),
            PackageType::Binary => self.installation_status.binary_available(),
        }
    }

    /// We found the dependency from the lockfile
    pub fn from_locked_package(
        package: &'d LockedPackage,
        installation_status: InstallationStatus,
    ) -> Self {
        Self {
            name: Cow::Borrowed(&package.name),
            version: Cow::Borrowed(&package.version),
            source: package.source.clone(),
            dependencies: package
                .dependencies
                .iter()
                .map(|d| Cow::Borrowed(d.as_str()))
                .collect(),
            suggests: package
                .suggests
                .iter()
                .map(|s| Cow::Borrowed(s.as_str()))
                .collect(),
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
        }
    }

    // TODO: 2 bool not great but maybe ok if it's only used in one place
    pub fn from_package_repository(
        package: &'d Package,
        repo_url: &str,
        package_type: PackageType,
        install_suggests: bool,
        force_source: bool,
        installation_status: InstallationStatus,
    ) -> (Self, InstallationDependencies<'d>) {
        let deps = package.dependencies_to_install(install_suggests);

        let res = Self {
            name: Cow::Borrowed(&package.name),
            version: Cow::Borrowed(&package.version.original),
            source: Source::Repository {
                repository: repo_url.to_string(),
            },
            dependencies: deps
                .direct
                .iter()
                .map(|d| Cow::Borrowed(d.name()))
                .collect(),
            suggests: deps
                .suggests
                .iter()
                .map(|d| Cow::Borrowed(d.name()))
                .collect(),
            kind: package_type,
            force_source,
            install_suggests,
            path: package.path.as_ref().map(|x| Cow::Borrowed(x.as_str())),
            from_lockfile: false,
            installation_status,
            remotes: HashMap::new(),
            from_remote: false,
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
            dependencies: deps
                .direct
                .iter()
                .map(|d| Cow::Owned(d.name().to_string()))
                .collect(),
            suggests: deps
                .suggests
                .iter()
                .map(|s| Cow::Owned(s.name().to_string()))
                .collect(),
            kind: PackageType::Source,
            force_source: true,
            path: None,
            from_lockfile: false,
            name: Cow::Owned(package.name.clone()),
            version: Cow::Owned(package.version.original.clone()),
            source,
            installation_status,
            install_suggests,
            remotes: package.remotes.clone(),
            from_remote: false,
        };

        (res, deps)
    }
}

impl<'a> fmt::Display for ResolvedDependency<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}={} ({}, type={}, path='{}', from_lockfile={}, from_remote={})",
            self.name,
            self.version,
            self.source,
            self.kind,
            self.path.as_deref().unwrap_or(""),
            self.from_lockfile,
            self.from_remote,
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
}

impl<'d> UnresolvedDependency<'d> {
    pub fn is_listed_in_config(&self) -> bool {
        self.parent.is_none()
    }
}

impl<'a> fmt::Display for UnresolvedDependency<'a> {
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
