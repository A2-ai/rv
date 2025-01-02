use std::path::PathBuf;

#[derive(Debug, PartialEq, Clone)]
pub enum CacheEntry {
    Existing(PathBuf),
    NotFound(PathBuf),
    Expired(PathBuf),
}

pub trait Cache {
    /// This will either load the database for that repository or return None if we couldn't find
    /// it or it was expired.
    fn get_package_db_entry(&self, repo_url: &str) -> CacheEntry;
}
