use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use filetime::FileTime;
use fs_err as fs;
use url::Url;

use crate::cache::InstallationStatus;
use crate::cache::utils::{
    get_current_system_path, get_packages_timeout, get_user_cache_dir, hash_string,
};
use crate::consts::{BUILD_LOG_FILENAME, BUILT_FROM_SOURCE_FILENAME};
use crate::lockfile::Source;
use crate::package::{BuiltinPackages, Package, get_builtin_versions_from_library};
use crate::system_req::get_system_requirements;
use crate::{RCmd, SystemInfo, Version};

#[derive(Debug, Clone)]
pub struct PackagePaths {
    pub binary: PathBuf,
    pub source: PathBuf,
}

/// This cache doesn't load anything, it just gets paths to cached objects.
/// Cache freshness is checked when requesting a path and is only a concern for package databases.
#[derive(Debug, Clone)]
pub struct DiskCache {
    /// The cache root directory.
    /// In practice, it will be the OS own cache specific directory + `rv`
    pub root: PathBuf,
    /// R version stored as [major, minor]
    pub r_version: [u32; 2],
    /// The current execution system info: OS, version etc.
    /// Needed for binaries
    pub system_info: SystemInfo,
    /// How long the compiled databases are considered fresh for, in seconds
    /// Defaults to 3600s (1 hour)
    packages_timeout: u64,
    readonly: bool,
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

    pub fn mark_readonly(mut self) -> Self {
        self.readonly = true;
        self
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
            readonly: false,
        })
    }

    /// PACKAGES databases as well as binary packages are dependent on the OS and R version
    pub(super) fn get_repo_root_binary_dir(&self, name: &str) -> PathBuf {
        let encoded = hash_string(name);
        self.root
            .join(&encoded)
            .join(get_current_system_path(&self.system_info, self.r_version))
    }

    /// A database contains both source and binary PACKAGE data
    /// Therefore the path to the db file is dependent on the system info and R version
    /// In practice it looks like: `CACHE_DIR/rv/{os}/{distrib?}/{arch?}/r_maj.r_min/{PACKAGE_DB_FILENAME}`
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

    /// Gets the folder where the R build package stdout+stderr output should be stored
    pub(crate) fn get_build_log_path(
        &self,
        source: &Source,
        pkg_name: Option<&str>,
        version: Option<&str>,
    ) -> PathBuf {
        let (parent_name, sha) = match source {
            Source::RUniverse { git, sha, .. } | Source::Git { git, sha, .. } => {
                (hash_string(git.url()), Some(sha.clone()))
            }
            Source::Url { url, sha, .. } => (hash_string(url.as_str()), Some(sha.clone())),
            Source::Repository { repository } => (hash_string(repository.as_str()), None),
            Source::Local { path, sha, .. } => (
                hash_string(path.as_os_str().to_string_lossy().as_ref()),
                sha.clone(),
            ),
            Source::Builtin { .. } => unreachable!(),
        };

        let mut p = self
            .root
            .join("logs")
            .join(&parent_name)
            .join(get_current_system_path(&self.system_info, self.r_version));

        if let Some(pkg_name) = pkg_name {
            p = p.join(pkg_name);
        }

        if let Some(version) = version.map(|x| x.to_string()).or(sha) {
            p = p.join(version);
        }

        p.join(BUILD_LOG_FILENAME)
    }

    /// Gets the folder where extracted source would be located
    /// The folder may or may not exist depending on whether it's in the cache
    fn get_source_package_path(&self, repo_url: &str, name: &str, version: &str) -> PathBuf {
        let encoded = hash_string(repo_url);
        self.root.join(encoded).join("src").join(name).join(version)
    }

    /// Gets where the source tarballs are saved when this option is enabled
    pub(super) fn get_source_tarball_folder(&self) -> PathBuf {
        self.root.join("source_tarballs")
    }

    /// Gets the path where a source tarball should be saved
    pub(crate) fn get_tarball_path(&self, name: &str, version: &str) -> PathBuf {
        self.get_source_tarball_folder()
            .join(format!("{name}_{version}.tar.gz"))
    }

    /// We will download them in a separate path, we don't know if we have source or binary
    pub(crate) fn get_url_download_path(&self, url: &Url) -> PathBuf {
        let encoded = hash_string(&url.as_str().to_ascii_lowercase());
        self.root.join("urls").join(encoded)
    }

    pub(crate) fn get_git_clone_path(&self, repo_url: &str) -> PathBuf {
        let encoded = hash_string(&repo_url.trim_end_matches("/").to_ascii_lowercase());
        self.root.join("git").join(encoded)
    }

    /// Search the cache for the related package db file.
    /// If it's not found or the entry is too old, the bool param will be false
    pub fn get_package_db_entry(&self, repo_url: &str) -> (PathBuf, bool) {
        let path = self.get_package_db_path(repo_url);

        if path.exists() {
            let metadata = path.metadata().expect("to work");
            let created = FileTime::from_last_modification_time(&metadata).unix_seconds() as u64;
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            return if (now - created) > self.packages_timeout {
                (path, false)
            } else {
                (path, true)
            };
        }

        (path, false)
    }

    pub(crate) fn get_package_paths(
        &self,
        source: &Source,
        pkg_name: Option<&str>,
        version: Option<&str>,
    ) -> PackagePaths {
        match source {
            Source::Git { git, sha, .. } => PackagePaths {
                source: self.get_git_clone_path(git.url()),
                binary: self.get_repo_root_binary_dir(git.url()).join(&sha[..10]),
            },
            Source::RUniverse { git, sha, .. } => PackagePaths {
                source: self.get_git_clone_path(git.url()),
                binary: self.get_repo_root_binary_dir(git.url()).join(&sha[..10]),
            },
            Source::Url { url, sha } => PackagePaths {
                source: self.get_url_download_path(url).join(&sha[..10]),
                binary: self.get_repo_root_binary_dir(url.as_str()).join(&sha[..10]),
            },
            Source::Repository { repository } => {
                let name = pkg_name.unwrap();
                let ver = version.unwrap();
                PackagePaths {
                    source: self.get_source_package_path(repository.as_str(), name, ver),
                    binary: self.get_binary_package_path(repository.as_str(), name, ver),
                }
            }
            Source::Local { .. } => unreachable!("Not used for local paths"),
            Source::Builtin { .. } => unreachable!("Not used for builtin packages"),
        }
    }

    /// Finds where a package is present in the cache depending on its source.
    /// The version param is only used when the source is a repository
    pub(crate) fn get_installation_status(
        &self,
        pkg_name: &str,
        version: &str,
        source: &Source,
    ) -> InstallationStatus {
        let (source_path, binary_path) = match source {
            Source::Git { .. } | Source::Url { .. } | Source::RUniverse { .. } => {
                let paths = self.get_package_paths(source, None, None);
                (paths.source, paths.binary.join(pkg_name))
            }
            Source::Repository { .. } => {
                let paths = self.get_package_paths(source, Some(pkg_name), Some(version));
                (paths.source.join(pkg_name), paths.binary.join(pkg_name))
            }
            // TODO: can we cache local somehow?
            Source::Local { .. } => return InstallationStatus::Absent,
            // TODO: check if we have specific versions
            Source::Builtin { .. } => return InstallationStatus::Binary(false),
        };

        let from_source = if binary_path.is_dir() {
            binary_path.join(BUILT_FROM_SOURCE_FILENAME).exists()
        } else {
            false
        };

        match (source_path.is_dir(), binary_path.is_dir()) {
            (true, true) => InstallationStatus::Both(from_source),
            (true, false) => InstallationStatus::Source,
            (false, true) => InstallationStatus::Binary(from_source),
            (false, false) => InstallationStatus::Absent,
        }
    }

    pub(super) fn get_builtin_packages_versions(
        &self,
        r_cmd: &impl RCmd,
    ) -> std::io::Result<HashMap<String, Package>> {
        let version = r_cmd.version().expect("to work");
        let filename = format!("builtin-{}.mp", version.original);
        let path = self.root.join(&filename);
        if let Some(builtin) = BuiltinPackages::load(&path) {
            Ok(builtin.packages)
        } else if self.readonly {
            Ok(HashMap::new())
        } else {
            let builtin = get_builtin_versions_from_library(r_cmd)?;
            builtin.persist(&path)?;
            Ok(builtin.packages)
        }
    }

    pub(super) fn get_system_requirements(&self) -> HashMap<String, Vec<String>> {
        let (distrib, version) = self.system_info.sysreq_data();
        let key = format!("sysreq-{distrib}-{version}.json",);
        let path = self.root.join(&key);
        // TODO: Handle expiration, what would be a reasonable time?
        if path.exists() {
            let content = fs::read_to_string(&path).expect("to work");
            serde_json::from_str(&content).unwrap()
        } else if self.readonly {
            HashMap::new()
        } else {
            let sysreq = get_system_requirements(&self.system_info);
            let content = serde_json::to_string(&sysreq).unwrap();
            fs::write(&path, content).expect("to work");
            sysreq
        }
    }
}
