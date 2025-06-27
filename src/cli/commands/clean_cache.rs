use core::fmt;
use std::{collections::HashMap, path::PathBuf};

use crate::{
    Resolution, UnresolvedDependency, Version,
    cli::{CliContext, context::load_databases},
    hash_string,
    package::PackageType,
};

use anyhow::Result;
use fs_err as fs;
use serde::Serialize;

/// Remove repositories and/or dependencies from the cache. 
/// Dependencies only remove the package version from the resolved source
/// Repositories are aliases corresponding to repos in the config
pub fn purge_cache<'a>(
    context: &'a CliContext,
    resolution: &'a Resolution<'a>,
    repositories: &'a [String],
    dependencies: &'a [String],
) -> std::io::Result<PurgeResults<'a>> {
    let mut repo_res = Vec::new();
    for r in repositories {
        let res = if let Some(repo) = context
            .config
            .repositories()
            .iter()
            .find(|repo| &repo.alias == r)
        {
            let path = context.cache.root.join(hash_string(repo.url()));
            if path.exists() {
                fs::remove_dir_all(&path)?;
                PurgeRepoResult::Removed {
                    alias: &repo.alias,
                    url: repo.url(),
                    path,
                }
            } else {
                PurgeRepoResult::NotInCache {
                    alias: &repo.alias,
                    url: repo.url(),
                }
            }
        } else {
            PurgeRepoResult::NotInProject(&r)
        };
        repo_res.push(res);
    }

    let mut dep_res = Vec::new();
    for d in dependencies {
        let res = if let Some(dep) = resolution.found.iter().find(|r| &r.name == d) {
            let (binary_path, source_path) = context.cache.remove_dependency(&dep.name, &dep.version, &dep.source)?;

            let mut paths = HashMap::new();
            if let Some(bin_path) = binary_path {
                paths.insert(PackageType::Binary, bin_path);
            }
            if let Some(src_path) = source_path {
                paths.insert(PackageType::Source, src_path);
            }

            if paths.is_empty() {
                PurgeDepResult::NotInCache {
                    name: &dep.name,
                    version: &dep.version,
                }
            } else {
                PurgeDepResult::Removed {
                    name: &dep.name,
                    version: &dep.version,
                    paths,
                }
            }
        } else if let Some(dep) = resolution.failed.iter().find(|r| &r.name == d) {
            PurgeDepResult::Unresolved(dep)
        } else {
            PurgeDepResult::NotInProject(d)
        };
        dep_res.push(res);
    }

    Ok(PurgeResults {
        repositories: repo_res,
        dependencies: dep_res,
    })
}

#[derive(Debug, Serialize)]
pub struct PurgeResults<'a> {
    repositories: Vec<PurgeRepoResult<'a>>,
    dependencies: Vec<PurgeDepResult<'a>>,
}

impl fmt::Display for PurgeResults<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.repositories.is_empty() {
            write!(f, "== Repositories ==\n")?;
            let mut not_removed = Vec::new();
            let mut removed = Vec::new();
            for repo in &self.repositories {
                match repo {
                    PurgeRepoResult::NotInProject(alias) => not_removed.push(format!(
                        "{alias} - Repository alias not found in config file"
                    )),
                    PurgeRepoResult::Removed { alias, url, .. } => {
                        removed.push(format!("{alias} ({url})"));
                    }
                    PurgeRepoResult::NotInCache { alias, url } => {
                        not_removed.push(format!("{alias} ({url}) - Repository not found in cache"))
                    }
                }
            }
            if !removed.is_empty() {
                write!(
                    f,
                    "Removed Successfully:\n    {}\n\n",
                    removed.join("\n    ")
                )?;
            }
            if !not_removed.is_empty() {
                write!(f, "Not Removed:\n    {}\n\n", not_removed.join("\n    "))?;
            }
        }

        if !self.dependencies.is_empty() {
            write!(f, "== Dependencies ==\n")?;
            let mut not_removed = Vec::new();
            let mut removed = Vec::new();
            let mut unresolved = Vec::new();
            for dep in &self.dependencies {
                match dep {
                    PurgeDepResult::Unresolved(dep) => {
                        unresolved.push(dep.to_string());
                        not_removed.push(format!("{} - Package could not be resolved", dep.name,));
                    }
                    PurgeDepResult::Removed {
                        name,
                        version,
                        paths,
                    } => {
                        let mut types = paths.keys().map(ToString::to_string).collect::<Vec<_>>();
                        types.sort();

                        removed.push(format!("{name} ({version}) - {}", types.join(" and ")))
                    }
                    PurgeDepResult::NotInCache { name, version } => not_removed.push(format!(
                        "{name} ({version}) - Dependency not found in cache"
                    )),
                    PurgeDepResult::NotInProject(name) => not_removed
                        .push(format!("{name} - Package not part of project dependencies")),
                }
            }

            if !removed.is_empty() {
                write!(
                    f,
                    "Removed Successfully:\n    {}\n\n",
                    removed.join("\n    ")
                )?;
            }
            if !not_removed.is_empty() {
                write!(f, "Not Removed:\n    {}\n\n", not_removed.join("\n    "))?;
            }
            if !unresolved.is_empty() {
                write!(
                    f,
                    "Failed to resolve all dependencies. Packages may not have been purged due to the following resolution issues:\n    {}\n\n",
                    unresolved.join("\n    ")
                )?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
enum PurgeRepoResult<'a> {
    Removed {
        alias: &'a str,
        url: &'a str,
        path: PathBuf,
    },
    /// Alias is not in config file
    NotInProject(&'a str),
    /// Repository not in cache (nothing to remove)
    NotInCache { alias: &'a str, url: &'a str },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
enum PurgeDepResult<'a> {
    /// Package is part of the unresolved dependencies
    Unresolved(&'a UnresolvedDependency<'a>),
    Removed {
        name: &'a str,
        version: &'a Version,
        paths: HashMap<PackageType, PathBuf>,
    },
    /// Dependency not part of the resolved dependency burden
    NotInProject(&'a str),
    /// Dependency resolved, but not in cache (nothing to remove)
    NotInCache { name: &'a str, version: &'a Version },
}

/// refresh the repository database by invalidating the packages.bin and re-loading it
/// returns a list of repositories that were refreshed and a list of repos that could not be found in the config
pub fn refresh_cache<'a>(
    context: &'a CliContext,
    repositories: &'a [String],
) -> Result<(Vec<RefreshedRepo<'a>>, Vec<&'a str>)> {
    let mut cache = context.cache.clone();
    // need to set cache timeout to refresh the databases for the found repositories
    cache.packages_timeout = 0;

    // if no repositories supplied, we'll refresh all repos listed in the config
    let res = if repositories.is_empty() {
        let res = context
            .config
            .repositories()
            .iter()
            .map(|repo| RefreshedRepo {
                alias: &repo.alias,
                url: repo.url(),
                path: cache.get_package_db_entry(repo.url()).0,
            })
            .collect::<Vec<_>>();

        load_databases(context.config.repositories(), &cache)?;
        (res, Vec::new())
    } else {
        let mut repos = Vec::new();
        let mut refreshed = Vec::new();
        // unresolved meaning that we can't find a corresponding entry in the config
        let mut unresolved = Vec::new();

        for r in repositories {
            if let Some(repo) = context
                .config
                .repositories()
                .iter()
                .find(|repo| &repo.alias == r)
            {
                repos.push(repo.clone());
                let (path, _) = cache.get_package_db_entry(repo.url());
                refreshed.push(RefreshedRepo {
                    alias: &repo.alias,
                    url: repo.url(),
                    path,
                });
            } else {
                unresolved.push(r.as_str());
            }
        }

        load_databases(&repos, &cache)?;
        (refreshed, unresolved)
    };

    Ok(res)
}

#[derive(Debug, Clone, Serialize)]
pub struct RefreshedRepo<'a> {
    alias: &'a str,
    url: &'a str,
    path: PathBuf,
}

impl fmt::Display for RefreshedRepo<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.alias, self.url)
    }
}
