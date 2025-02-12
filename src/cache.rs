use std::fmt;
use std::fmt::Formatter;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Clone)]
pub enum CacheEntry {
    Existing(PathBuf),
    NotFound(PathBuf),
    Expired(PathBuf),
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

pub trait Cache {
    /// This will either load the database for that repository or return None if we couldn't find
    /// it or it was expired.
    fn get_package_db_entry(&self, repo_url: &str) -> CacheEntry;

    /// Gets the status of a package coming from a package repository in the cache
    fn get_package_installation_status(
        &self,
        repo_url: &str,
        name: &str,
        version: &str,
    ) -> InstallationStatus;

    fn get_git_installation_status(&self, repo_url: &str, sha: &str) -> InstallationStatus;

    /// Gets the path to where a git repository should be cloned
    fn get_git_clone_path(&self, repo_url: &str) -> PathBuf;

    /// Gets the path to where a tarball package should be downloaded
    fn get_url_download_path(&self, url: &str) -> PathBuf;
}
