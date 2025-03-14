use serde::Deserialize;

use crate::cache::InstallationStatus;
use crate::http::HttpError;
use crate::lockfile::{LockedPackage, Source};
use crate::package::{InstallationDependencies, Package, PackageRemote, PackageType};
use crate::resolver::QueueItem;
use crate::{Http, HttpDownload, Version, VersionRequirement};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::str::{FromStr, Utf8Error};

/// A dependency that we found from any of the sources we can look up to
/// We use Cow everywhere because only for git/local packages will be owned, the vast majority
/// will be borrowed
#[derive(PartialEq, Clone)]
pub struct ResolvedDependency<'d> {
    pub(crate) name: Cow<'d, str>,
    pub(crate) version: Cow<'d, Version>,
    pub(crate) source: Source,
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
    // Only set for local dependencies. This is the full resolved path to a directory/tarball
    pub(crate) local_resolved_path: Option<PathBuf>,
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

    /// We found the dependency from the lockfile
    pub fn from_locked_package(
        package: &'d LockedPackage,
        installation_status: InstallationStatus,
    ) -> Self {
        Self {
            name: Cow::Borrowed(&package.name),
            version: Cow::Owned(Version::from_str(package.version.as_str()).unwrap()),
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
            local_resolved_path: None,
        }
    }

    pub fn from_package_repository(
        package: &'d Package,
        repo_url: &str,
        package_type: PackageType,
        install_suggests: bool,
        force_source: bool,
        installation_status: InstallationStatus,
    ) -> (Self, InstallationDependencies<'d>) {
        let deps = package.dependencies_to_install(install_suggests);

        let mut source = Source::Repository {
            repository: repo_url.to_string(),
        };
        // If repository is r-universe, treat as a git repo since r-universe does not have archive

        // TODO: If/when the need arises to not treat r-universe as a git repo, the potential spec is to keep both the repository 
        // and git info. The repository is used while the locked version and version in the PACKAGES database match, 
        // switching to using git once it is no longer available
        if repo_url.contains("r-universe.dev") {
            match RUniverseApi::query_r_universe_api(package, repo_url) {
                Ok(r) => source = Source::Git { git: r.remote_url, sha: r.remote_sha, directory: None, tag: None, branch: None },
                Err(e) => log::warn!("Could not properly lock {} due to: {e:?}. Falling back to standard repository. Library may not be able to be recreated.", package.name),
            }
        }

        let res = Self {
            name: Cow::Borrowed(&package.name),
            version: Cow::Borrowed(&package.version),
            source,
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
            local_resolved_path: None,
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
            version: Cow::Owned(package.version.clone()),
            source,
            installation_status,
            install_suggests,
            remotes: package.remotes.clone(),
            from_remote: false,
            local_resolved_path: None,
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
            version: Cow::Owned(package.version.clone()),
            source,
            // We'll handle the installation status later by comparing mtimes
            installation_status: InstallationStatus::Source,
            install_suggests,
            remotes: package.remotes.clone(),
            from_remote: false,
            local_resolved_path: Some(local_resolved_path),
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
            kind,
            force_source: false,
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
            local_resolved_path: None,
        };

        (res, deps)
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for ResolvedDependency<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}={} ({:?}, type={}, path='{}', from_lockfile={}, from_remote={})",
            self.name,
            self.version.original,
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

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RUniverseApi {
    remote_url: String,
    remote_sha: String,
}

impl RUniverseApi {
    fn query_r_universe_api(package: &Package, repo_url: &str) -> Result<Self, RUniverseApiError> {
        let http = Http {};
        let api_url = format!("{}/api/packages/{}", repo_url, package.name);
        let mut writer = Vec::new();

        http.download(&api_url, &mut writer, Vec::new())?;
        let r_universe_api: RUniverseApi = serde_json::from_slice(&writer)?;

        Ok(r_universe_api)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to determine Git info from R-Universe API")]
#[non_exhaustive]
pub struct RUniverseApiError {
    pub source: RUniverseApiErrorKind,
}

impl From<HttpError> for RUniverseApiError {
    fn from(value: HttpError) -> Self {
        Self {
            source: RUniverseApiErrorKind::Http(value),
        }
    }
}

impl From<Utf8Error> for RUniverseApiError {
    fn from(value: Utf8Error) -> Self {
        Self {
            source: RUniverseApiErrorKind::Utf8(value),
        }
    }
}

impl From<serde_json::Error> for RUniverseApiError {
    fn from(value: serde_json::Error) -> Self {
        Self {
            source: RUniverseApiErrorKind::Parse(value),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RUniverseApiErrorKind {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Utf8(#[from] Utf8Error),
    #[error(transparent)]
    Parse(#[from] serde_json::Error),
}