use std::path::PathBuf;

#[derive(Debug, PartialEq, Clone)]
pub enum CacheEntry {
    Existing(PathBuf),
    NotFound(PathBuf),
}

pub trait Cache {
    /// This will either load the database for that repository or return None if we couldn't find
    /// it or it was expired. If it was expired, it will also be deleted from disk.
    fn get_package_db_entry(&self, repo_url: &str) -> CacheEntry;
}
