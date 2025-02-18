use std::fmt;
use std::path::PathBuf;

use serde::Serialize;

use crate::cli::CliContext;
use crate::lockfile::Source;
use crate::{hash_string, ResolvedDependency};

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
}

impl fmt::Display for CacheRepositoryInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({} -> {}), path: {}",
            self.alias,
            self.url,
            self.hash,
            self.path.display()
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
    pub fn new(context: &CliContext, resolved: Vec<ResolvedDependency>) -> Self {
        let root = context.cache.root.clone();
        let repositories = context
            .config
            .repositories()
            .iter()
            .map(|r| {
                let hash = hash_string(r.url());
                CacheRepositoryInfo {
                    alias: r.alias.clone(),
                    url: r.url().to_string(),
                    path: root.join(hash_string(r.url())),
                    hash,
                }
            })
            .collect();

        let mut git_paths = Vec::new();
        let mut url_paths = Vec::new();

        for d in resolved {
            match d.source {
                Source::Git { git, sha, .. } => {
                    let paths = context.cache.get_git_package_paths(&git, &sha);
                    git_paths.push(CacheUrlInfo {
                        url: git,
                        source_path: paths.source,
                        binary_path: paths.binary,
                    });
                }
                Source::Url { url, sha, .. } => {
                    let paths = context.cache.get_url_package_paths(&url, &sha);
                    url_paths.push(CacheUrlInfo {
                        url,
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
        write!(f, "{}\n", self.root.display())?;
        for r in &self.repositories {
            write!(f, "{}\n", r)?;
        }
        for r in &self.git {
            write!(f, "Git: {}\n", r)?;
        }
        for r in &self.urls {
            write!(f, "Url: {}\n", r)?;
        }

        Ok(())
    }
}
