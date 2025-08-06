use std::fmt;
use std::path::PathBuf;

use crate::lockfile::Source;
use crate::{Config, DiskCache, ResolvedDependency, hash_string};
use serde::Serialize;

/// Both for git and remote urls
#[derive(Debug, Serialize)]
struct CacheUrlInfo {
    url: String,
    source_path: PathBuf,
    binary_path: PathBuf,
}
impl fmt::Display for CacheUrlInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}, source: {}, binary: {}",
            self.url,
            self.source_path.display(),
            self.binary_path.display()
        )
    }
}

#[derive(Debug, Serialize)]
struct CacheRepositoryInfo {
    alias: String,
    url: String,
    hash: String,
    path: PathBuf,
    binary_path: PathBuf,
}

impl fmt::Display for CacheRepositoryInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} -> {}), path: {}, binary path: {}",
            self.alias,
            self.url,
            self.hash,
            self.path.display(),
            self.binary_path.display(),
        )
    }
}

#[derive(Debug, Serialize)]
pub struct CacheInfo {
    root: PathBuf,
    repositories: Vec<CacheRepositoryInfo>,
    git: Vec<CacheUrlInfo>,
    urls: Vec<CacheUrlInfo>,
}

impl CacheInfo {
    pub fn new(config: &Config, cache: &DiskCache, resolved: Vec<ResolvedDependency>) -> Self {
        let root = cache.root.clone();
        let repositories = config
            .repositories()
            .iter()
            .map(|r| {
                let binary_path = cache.get_repo_root_binary_dir(r.url());
                let path = binary_path
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_path_buf();
                let hash = hash_string(r.url());
                CacheRepositoryInfo {
                    alias: r.alias.clone(),
                    url: r.url().to_string(),
                    path,
                    binary_path,
                    hash,
                }
            })
            .collect();

        let mut git_paths = Vec::new();
        let mut url_paths = Vec::new();

        for d in resolved {
            if !d.source.is_git_or_url() {
                continue;
            }
            let paths = cache.get_package_paths(&d.source, None, None);
            match d.source {
                Source::Git { git, .. } => {
                    git_paths.push(CacheUrlInfo {
                        url: git.url().to_string(),
                        source_path: paths.source,
                        binary_path: paths.binary,
                    });
                }
                Source::RUniverse { git, .. } => {
                    git_paths.push(CacheUrlInfo {
                        url: git.url().to_string(),
                        source_path: paths.source,
                        binary_path: paths.binary,
                    });
                }
                Source::Url { url, .. } => {
                    url_paths.push(CacheUrlInfo {
                        url: url.as_str().to_string(),
                        source_path: paths.source,
                        binary_path: paths.binary,
                    });
                }
                _ => continue,
            }
        }

        Self {
            root,
            repositories,
            git: git_paths,
            urls: url_paths,
        }
    }
}

impl fmt::Display for CacheInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.root.display())?;
        for r in &self.repositories {
            writeln!(f, "{}", r)?;
        }
        for r in &self.git {
            writeln!(f, "Git: {}", r)?;
        }
        for r in &self.urls {
            writeln!(f, "Url: {}", r)?;
        }

        Ok(())
    }
}
