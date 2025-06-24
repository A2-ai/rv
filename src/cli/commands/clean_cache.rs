use core::fmt;
use std::{collections::HashMap, path::PathBuf};

use crate::{ResolvedDependency, Version, cli::CliContext, hash_string, package::PackageType};

use fs_err as fs;
use serde::Serialize;

pub fn purge_cache<'a>(
    context: &'a CliContext,
    resolved: &'a [ResolvedDependency<'a>],
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
                PurgeRepoResult::NotFound {
                    alias: &repo.alias,
                    url: repo.url(),
                }
            }
        } else {
            PurgeRepoResult::Unresolved(&r)
        };
        repo_res.push(res);
    }

    let mut dep_res = Vec::new();
    for d in dependencies {
        let res = if let Some(dep) = resolved.iter().find(|r| &r.name == d) {
            let pkg_paths = context.cache.get_package_paths(
                &dep.source,
                Some(&dep.name),
                Some(&dep.version.original),
            );
            let mut paths = HashMap::new();
            if pkg_paths.binary.exists() {
                fs::remove_dir_all(&pkg_paths.binary)?;
                paths.insert(PackageType::Binary, pkg_paths.binary);
            }

            if pkg_paths.source.exists() {
                fs::remove_dir_all(&pkg_paths.source)?;
                paths.insert(PackageType::Source, pkg_paths.source);
            }

            if paths.is_empty() {
                PurgeDepResult::NotFound {
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
        } else {
            PurgeDepResult::Unresolved(d)
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
                    PurgeRepoResult::Unresolved(alias) => not_removed.push(format!(
                        "{alias} - Repository alias not found in config file"
                    )),
                    PurgeRepoResult::Removed { alias, url, .. } => {
                        removed.push(format!("{alias} ({url})"));
                    }
                    PurgeRepoResult::NotFound { alias, url } => {
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
            for dep in &self.dependencies {
                match dep {
                    PurgeDepResult::Unresolved(name) => not_removed.push(format!(
                        "{name} - Dependency not found in resolved packages"
                    )),
                    PurgeDepResult::Removed {
                        name,
                        version,
                        paths,
                    } => {
                        let mut types = paths.keys().map(ToString::to_string).collect::<Vec<_>>();
                        types.sort();

                        removed.push(format!("{name} ({version}) - {}", types.join(" and ")))
                    }
                    PurgeDepResult::NotFound { name, version } => not_removed.push(format!(
                        "{name} ({version}) - Dependency not found in cache"
                    )),
                }
            }

            if !removed.is_empty() {
                write!(
                    f,
                    "The following dependencies were successfully removed:\n    {}\n\n",
                    removed.join("\n    ")
                )?;
            }
            if !not_removed.is_empty() {
                write!(
                    f,
                    "The following dependencies were not removed:\n    {}\n",
                    not_removed.join("\n    ")
                )?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
enum PurgeRepoResult<'a> {
    Unresolved(&'a str),
    Removed {
        alias: &'a str,
        url: &'a str,
        path: PathBuf,
    },
    NotFound {
        alias: &'a str,
        url: &'a str,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
enum PurgeDepResult<'a> {
    Unresolved(&'a str),
    Removed {
        name: &'a str,
        version: &'a Version,
        paths: HashMap<PackageType, PathBuf>,
    },
    NotFound {
        name: &'a str,
        version: &'a Version,
    },
}
