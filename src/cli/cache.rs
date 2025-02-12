use std::path::PathBuf;
use std::time::SystemTime;

use crate::cache::InstallationStatus;
use crate::cli::utils::get_current_system_path;
use crate::system_info::SystemInfo;
use crate::Version;
use crate::{Cache, CacheEntry};
use anyhow::{anyhow, Context, Result};
use etcetera::BaseStrategy;
use fs_err as fs;
use sha2::{Digest, Sha256};

/// How long are the package databases cached for
/// Same default value as PKGCACHE_TIMEOUT:
/// https://github.com/r-lib/pkgcache?tab=readme-ov-file#package-environment-variables
const PACKAGE_TIMEOUT: u64 = 60 * 60;
const PACKAGE_TIMEOUT_ENV_VAR_NAME: &str = "PKGCACHE_TIMEOUT";
const PACKAGE_DB_FILENAME: &str = "packages.bin";

fn get_user_cache_dir() -> Option<PathBuf> {
    etcetera::base_strategy::choose_base_strategy()
        .ok()
        .map(|dirs| dirs.cache_dir().join("rv"))
}

#[inline]
fn get_packages_timeout() -> u64 {
    if let Ok(v) = std::env::var(PACKAGE_TIMEOUT_ENV_VAR_NAME) {
        if let Ok(v2) = v.parse() {
            v2
        } else {
            // If the variable doesn't parse into a valid number, return the default one
            PACKAGE_TIMEOUT
        }
    } else {
        PACKAGE_TIMEOUT
    }
}

#[inline]
fn encode_path(url: &str, name: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let result = format!("{:x}", hasher.finalize());
    if let Some(n) = name {
        format!("{}-{}", n.chars().filter(|c| c.is_ascii()).collect::<String>().to_ascii_lowercase(), &result[..10])
    } else {
        format!("{}", &result[..10])
    }
}

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
    root: PathBuf,
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
    pub fn new(r_version: &Version, system_info: SystemInfo) -> Result<Self> {
        let root =
            get_user_cache_dir().ok_or_else(|| anyhow!("Could not get user cache directory"))?;
        fs::create_dir_all(&root)?;
        cachedir::ensure_tag(&root).context("Failed to create CACHEDIR.TAG")?;

        Ok(Self {
            root,
            system_info,
            r_version: r_version.major_minor(),
            packages_timeout: get_packages_timeout(),
        })
    }

    /// PACKAGES databases as well as binary packages are dependent on the OS and R version
    fn get_repo_root_binary_dir(&self, repo_name: Option<&str>, repo_url: &str) -> PathBuf {
        let encoded = encode_path(repo_url, repo_name);
        self.root
            .join(&encoded)
            .join(get_current_system_path(&self.system_info, self.r_version))
    }

    /// A database contains both source and binary PACKAGE data
    /// Therefore the path to the db file is dependent on the system info and R version
    /// In practice it looks like: `CACHE_DIR/rv/{os}/{distrib?}/{arch?}/r_maj.r_min/packages.bin`
    fn get_package_db_path(&self, repo_name: &str, repo_url: &str) -> PathBuf {
        let base_path = self.get_repo_root_binary_dir(Some(repo_name), repo_url);
        base_path.join(PACKAGE_DB_FILENAME)
    }

    /// Gets the folder where a binary package would be located.
    /// The folder may or may not exist depending on whether it's in the cache
    pub fn get_binary_package_path(
        &self,
        repo_name: Option<&str>,
        repo_url: &str,
        name: &str,
        version: &str,
    ) -> PathBuf {
        self.get_repo_root_binary_dir(repo_name, repo_url)
            .join(name)
            .join(version)
    }

    /// Gets the folder where a source tarball would be located
    /// The folder may or may not exist depending on whether it's in the cache
    pub fn get_source_package_path(
        &self,
        repo_name: Option<&str>,
        repo_url: &str,
        name: &str,
        version: &str,
    ) -> PathBuf {
        let encoded = encode_path(repo_url, repo_name);
        self.root.join(encoded).join("src").join(name).join(version)
    }

    pub fn get_package_paths(
        &self,
        repo_name: Option<&str>,
        repo_url: &str,
        name: &str,
        version: &str,
    ) -> PackagePaths {
        PackagePaths {
            source: self.get_source_package_path(repo_name, repo_url, name, version),
            binary: self.get_binary_package_path(repo_name, repo_url, name, version),
        }
    }

    /// We will download them in a separate path, we don't know if we have source or binary
    fn get_url_path(&self, url: &str) -> PathBuf {
        let encoded = encode_path(url, None);
        self.root.join("urls").join(encoded)
    }

    fn get_source_git_package_path(&self, repo_url: &str) -> PathBuf {
        let encoded = encode_path(repo_url, None);
        self.root.join("git").join(encoded)
    }

    pub fn get_git_package_paths(&self, repo_url: &str, sha: &str) -> PackagePaths {
        PackagePaths {
            source: self.get_source_git_package_path(repo_url),
            binary: self.get_repo_root_binary_dir(None, repo_url).join(sha),
        }
    }

    pub fn get_git_build_path(&self, repo_url: &str, sha: &str) -> PathBuf {
        let encoded = encode_path(repo_url, None);
        self.root.join("git").join("builds").join(encoded).join(sha)
    }

    pub fn get_url_package_paths(&self, url: &str, sha: &str) -> PackagePaths {
        PackagePaths {
            source: self.get_url_path(url).join(&sha[..7]),
            binary: self.get_repo_root_binary_dir(None, url).join(sha),
        }
    }
}

impl Cache for DiskCache {
    fn get_package_db_entry(&self, repo_name: &str, repo_url: &str) -> CacheEntry {
        let path = self.get_package_db_path(repo_name, repo_url);
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
        repo_name: Option<&str>,
        repo_url: &str,
        name: &str,
        version: &str,
    ) -> InstallationStatus {
        let source_present = self
            .get_source_package_path(repo_name, repo_url, name, version)
            .join(name)
            .is_dir();
        let binary_present = self
            .get_binary_package_path(repo_name, repo_url, name, version)
            .join(name)
            .is_dir();

        match (source_present, binary_present) {
            (true, true) => InstallationStatus::Both,
            (true, false) => InstallationStatus::Source,
            (false, true) => InstallationStatus::Binary,
            (false, false) => InstallationStatus::Absent,
        }
    }

    fn get_git_installation_status(&self, repo_url: &str, sha: &str) -> InstallationStatus {
        let paths = self.get_git_package_paths(repo_url, sha);

        // TODO: check if we need to add the name for binary path
        match (paths.source.is_dir(), paths.binary.is_dir()) {
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
