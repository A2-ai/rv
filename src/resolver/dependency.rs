use std::borrow::Cow;
use std::fmt;

use crate::cache::InstallationStatus;
use crate::lockfile::{LockedPackage, Source};
use crate::package::{InstallationDependencies, Package, PackageType};
use crate::version::VersionRequirement;

/// A dependency that we found from any of the sources we can look up to
/// We use Cow everywhere because only for git/local packages will be owned, the vast majority
/// will be borrowed
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ResolvedDependency<'d> {
    pub(crate) name: Cow<'d, str>,
    pub(crate) version: Cow<'d, str>,
    pub(crate) source: Source,
    pub(crate) dependencies: Vec<Cow<'d, str>>,
    pub(crate) suggests: Vec<Cow<'d, str>>,
    pub(crate) force_source: bool,
    pub(crate) install_suggests: bool,
    pub(crate) kind: PackageType,
    pub(crate) installation_status: InstallationStatus,
    pub(crate) path: Option<Cow<'d, str>>,
    pub(crate) found_in_lockfile: bool,
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
            found_in_lockfile: true,
            installation_status,
        }
    }

    // TODO: 2 bool not great but maybe ok if it's only used in one place
    pub fn from_package_repository(
        package: &'d Package,
        repo_url: &str,
        package_type: PackageType,
        install_suggestions: bool,
        force_source: bool,
        installation_status: InstallationStatus,
    ) -> (Self, InstallationDependencies<'d>) {
        let deps = package.dependencies_to_install(install_suggestions);

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
            install_suggests: install_suggestions,
            path: package.path.as_ref().map(|x| Cow::Borrowed(x.as_str())),
            found_in_lockfile: false,
            installation_status,
        };

        (res, deps)
    }

    /// If we find the package to be a git repo, we will read the DESCRIPTION file during resolution
    /// This means the data will not outlive this struct and needs to be owned
    pub fn from_git_package<'p>(
        package: &'p Package,
        repo_url: &str,
        sha: String,
        directory: Option<String>,
        install_suggestions: bool,
        installation_status: InstallationStatus,
    ) -> (Self, InstallationDependencies<'p>) {
        let deps = package.dependencies_to_install(install_suggestions);

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
            install_suggests: install_suggestions,
            path: None,
            found_in_lockfile: false,
            name: Cow::Owned(package.name.clone()),
            version: Cow::Owned(package.version.original.clone()),
            source: Source::Git {
                git: repo_url.to_string(),
                directory,
                sha,
            },
            installation_status,
        };

        (res, deps)
    }
}

impl<'a> fmt::Display for ResolvedDependency<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}={} ({}, type={}, from_lockfile={}, path='{}')",
            self.name,
            self.version,
            self.source,
            self.kind,
            self.found_in_lockfile,
            self.path.as_deref().unwrap_or("")
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
