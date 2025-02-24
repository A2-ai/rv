use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fs_err as fs;

use crate::cache::utils::{
    get_current_system_path, get_packages_timeout, get_user_cache_dir, hash_string,
};
use crate::cache::InstallationStatus;
use crate::{Cache, CacheEntry, SystemInfo, Version};

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
    pub fn get_binary_package_path(&self, repo_url: &str, name: &str, version: &str) -> PathBuf {
        self.get_repo_root_binary_dir(repo_url)
            .join(name)
            .join(version)
    }

    /// Gets the folder where a source tarball would be located
    /// The folder may or may not exist depending on whether it's in the cache
    pub fn get_source_package_path(&self, repo_url: &str, name: &str, version: &str) -> PathBuf {
        let encoded = hash_string(repo_url);
        self.root.join(encoded).join("src").join(name).join(version)
    }

    pub fn get_package_paths(&self, repo_url: &str, name: &str, version: &str) -> PackagePaths {
        PackagePaths {
            source: self.get_source_package_path(repo_url, name, version),
            binary: self.get_binary_package_path(repo_url, name, version),
        }
    }

    /// We will download them in a separate path, we don't know if we have source or binary
    fn get_url_path(&self, url: &str) -> PathBuf {
        let encoded = hash_string(url);
        self.root.join("urls").join(encoded)
    }

    fn get_source_git_package_path(&self, repo_url: &str) -> PathBuf {
        let encoded = hash_string(repo_url);
        self.root.join("git").join(encoded)
    }

    pub fn get_git_package_paths(&self, repo_url: &str, sha: &str) -> PackagePaths {
        PackagePaths {
            source: self.get_source_git_package_path(repo_url),
            binary: self.get_repo_root_binary_dir(repo_url).join(&sha[..10]),
        }
    }

    pub fn get_git_build_path(&self, repo_url: &str, sha: &str) -> PathBuf {
        let encoded = hash_string(repo_url);
        self.root
            .join("git")
            .join("builds")
            .join(encoded)
            .join(&sha[..10])
    }

    pub fn get_url_package_paths(&self, url: &str, sha: &str) -> PackagePaths {
        PackagePaths {
            source: self.get_url_path(url).join(&sha[..10]),
            binary: self.get_repo_root_binary_dir(url).join(&sha[..10]),
        }
    }
}

impl Cache for DiskCache {
    fn get_package_db_entry(&self, repo_url: &str) -> CacheEntry {
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
                CacheEntry::Expired(path)
            } else {
                CacheEntry::Existing(path)
            };
        }

        CacheEntry::NotFound(path)
    }

    fn get_package_installation_status(
        &self,
        repo_url: &str,
        name: &str,
        version: &str,
    ) -> InstallationStatus {
        let source_present = self
            .get_source_package_path(repo_url, name, version)
            .join(name)
            .is_dir();
        let binary_present = self
            .get_binary_package_path(repo_url, name, version)
            .join(name)
            .is_dir();

        match (source_present, binary_present) {
            (true, true) => InstallationStatus::Both,
            (true, false) => InstallationStatus::Source,
            (false, true) => InstallationStatus::Binary,
            (false, false) => InstallationStatus::Absent,
        }
    }

    fn get_git_installation_status(
        &self,
        git_url: &str,
        sha: &str,
        pkg_name: &str,
    ) -> InstallationStatus {
        let paths = self.get_git_package_paths(git_url, sha);

        match (paths.source.is_dir(), paths.binary.join(pkg_name).is_dir()) {
            (true, true) => InstallationStatus::Both,
            (true, false) => InstallationStatus::Source,
            (false, true) => InstallationStatus::Binary,
            (false, false) => InstallationStatus::Absent,
        }
    }

    fn get_url_installation_status(
        &self,
        url: &str,
        sha: &str,
        pkg_name: &str,
    ) -> InstallationStatus {
        let paths = self.get_url_package_paths(url, sha);

        match (paths.source.is_dir(), paths.binary.join(pkg_name).is_dir()) {
            (true, true) => InstallationStatus::Both,
            (true, false) => InstallationStatus::Source,
            (false, true) => InstallationStatus::Binary,
            (false, false) => InstallationStatus::Absent,
        }
    }

    fn get_git_clone_path(&self, git_url: &str) -> PathBuf {
        self.get_source_git_package_path(&git_url)
    }

    fn get_url_download_path(&self, url: &str) -> PathBuf {
        self.get_url_path(&url)
    }
}
