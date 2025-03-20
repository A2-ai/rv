use std::error::Error;
use std::fmt;
use std::fmt::Formatter;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fs_err as fs;

use crate::cache::utils::{
    get_current_system_path, get_packages_timeout, get_user_cache_dir, hash_string,
};
use crate::lockfile::Source;
use crate::{SystemInfo, Version};

#[derive(Debug, Clone)]
pub struct PackagePaths {
    pub binary: PathBuf,
    pub source: PathBuf,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum InstallationStatus {
    Source,
    Binary,
    Both,
    Absent,
}

impl InstallationStatus {
    pub fn available(&self) -> bool {
        *self != InstallationStatus::Absent
    }

    pub fn binary_available(&self) -> bool {
        matches!(self, InstallationStatus::Binary | InstallationStatus::Both)
    }

    pub fn source_available(&self) -> bool {
        matches!(self, InstallationStatus::Source | InstallationStatus::Both)
    }
}

impl fmt::Display for InstallationStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let v = match self {
            InstallationStatus::Source => "source",
            InstallationStatus::Binary => "binary",
            InstallationStatus::Both => "source and binary",
            InstallationStatus::Absent => "absent",
        };
        write!(f, "{v}")
    }
}

/// This cache doesn't load anything, it just gets paths to cached objects.
/// Cache freshness is checked when requesting a path and is only a concern for package databases.
#[derive(Debug, Clone)]
pub struct DiskCache {
    /// The cache root directory.
    /// In practice it will be the OS own cache specific directory + `rv`
    pub root: PathBuf,
    /// R version stored as [major, minor]
    pub r_version: [u32; 2],
    /// The current execution system info: OS, version etc.
    /// Needed for binaries
    pub system_info: SystemInfo,
    /// How long the compiled databases are considered fresh for, in seconds
    /// Defaults to 3600s (1 hour)
    packages_timeout: u64,
    // TODO: check if it's worth keeping a hashmap of repo_url -> encoded
    // TODO: or if the overhead is the same as base64 directly
}

impl DiskCache {
    /// Instantiate our cache abstraction.
    pub fn new(
        r_version: &Version,
        system_info: SystemInfo,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let root = match get_user_cache_dir() {
            Some(path) => path,
            None => return Err("Could not find user cache directory".into()),
        };
        fs::create_dir_all(&root)?;
        if let Err(e) = cachedir::ensure_tag(&root) {
            return Err(format!("Failed to create CACHEDIR.TAG: {e}").into());
        }

        Self::new_in_dir(r_version, system_info, root)
    }

    pub(crate) fn new_in_dir(
        r_version: &Version,
        system_info: SystemInfo,
        root: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Ok(Self {
            root: root.as_ref().to_path_buf(),
            system_info,
            r_version: r_version.major_minor(),
            packages_timeout: get_packages_timeout(),
        })
    }

    /// PACKAGES databases as well as binary packages are dependent on the OS and R version
    fn get_repo_root_binary_dir(&self, name: &str) -> PathBuf {
        let encoded = hash_string(name);
        self.root
            .join(&encoded)
            .join(get_current_system_path(&self.system_info, self.r_version))
    }

    /// A database contains both source and binary PACKAGE data
    /// Therefore the path to the db file is dependent on the system info and R version
    /// In practice it looks like: `CACHE_DIR/rv/{os}/{distrib?}/{arch?}/r_maj.r_min/packages.bin`
    fn get_package_db_path(&self, repo_url: &str) -> PathBuf {
        let base_path = self.get_repo_root_binary_dir(repo_url);
        base_path.join(crate::consts::PACKAGE_DB_FILENAME)
    }

    /// Gets the folder where a binary package would be located.
    /// The folder may or may not exist depending on whether it's in the cache
    fn get_binary_package_path(&self, repo_url: &str, name: &str, version: &str) -> PathBuf {
        self.get_repo_root_binary_dir(repo_url)
            .join(name)
            .join(version)
    }

    /// Gets the folder where a source tarball would be located
    /// The folder may or may not exist depending on whether it's in the cache
    fn get_source_package_path(&self, repo_url: &str, name: &str, version: &str) -> PathBuf {
        let encoded = hash_string(repo_url);
        self.root.join(encoded).join("src").join(name).join(version)
    }

    /// We will download them in a separate path, we don't know if we have source or binary
    pub fn get_url_download_path(&self, url: &str) -> PathBuf {
        let encoded = hash_string(&url.to_ascii_lowercase());
        self.root.join("urls").join(encoded)
    }

    pub fn get_git_clone_path(&self, repo_url: &str) -> PathBuf {
        let encoded = hash_string(&repo_url.trim_end_matches("/").to_ascii_lowercase());
        self.root.join("git").join(encoded)
    }

    /// Search the cache for the related package db file.
    /// If it's not found or the entry is too old, the bool param will be false
    pub fn get_package_db_entry(&self, repo_url: &str) -> (PathBuf, bool) {
        let path = self.get_package_db_path(repo_url);
        if path.exists() {
            let created = path
                .metadata()
                .expect("to work")
                .created()
                .expect("to have a creation time");
            let now = SystemTime::now();

            return if now.duration_since(created).unwrap_or_default().as_secs()
                > self.packages_timeout
            {
                (path, false)
            } else {
                (path, true)
            };
        }

        (path, false)
    }

    pub fn get_package_paths(
        &self,
        source: &Source,
        pkg_name: Option<&str>,
        version: Option<&str>,
    ) -> PackagePaths {
        match source {
            Source::Git { git, sha, .. } => PackagePaths {
                source: self.get_git_clone_path(git),
                binary: self.get_repo_root_binary_dir(git).join(&sha[..10]),
            },
            Source::Url { url, sha } => PackagePaths {
                source: self.get_url_download_path(url).join(&sha[..10]),
                binary: self.get_repo_root_binary_dir(url).join(&sha[..10]),
            },
            Source::Repository { repository } => PackagePaths {
                source: self.get_source_package_path(
                    repository,
                    pkg_name.unwrap(),
                    version.unwrap(),
                ),
                binary: self.get_binary_package_path(
                    repository,
                    pkg_name.unwrap(),
                    version.unwrap(),
                ),
            },
            Source::Local { .. } => unreachable!("Not used for local paths"),
        }
    }

    /// Finds where a package is present in the cache depending on its source.
    /// The version param is only used when the source is a repository
    pub fn get_installation_status(
        &self,
        pkg_name: &str,
        version: &str,
        source: &Source,
    ) -> InstallationStatus {
        let (source_path, binary_path) = match source {
            Source::Git { .. } | Source::Url { .. } => {
                let paths = self.get_package_paths(source, None, None);
                (paths.source, paths.binary.join(pkg_name))
            }
            Source::Repository { .. } => {
                let paths = self.get_package_paths(source, Some(pkg_name), Some(version));
                (paths.source.join(pkg_name), paths.binary.join(pkg_name))
            }
            // TODO: can we cache local somehow?
            Source::Local { .. } => return InstallationStatus::Absent,
        };

        match (source_path.is_dir(), binary_path.is_dir()) {
            (true, true) => InstallationStatus::Both,
            (true, false) => InstallationStatus::Source,
            (false, true) => InstallationStatus::Binary,
            (false, false) => InstallationStatus::Absent,
        }
    }
}
