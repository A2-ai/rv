use std::{collections::HashMap, fmt, hash::Hash, path::Path};

use serde::{Serialize, Deserialize};

use crate::{cache::InstallationStatus, lockfile::Source, DiskCache, Library, Lockfile, RepositoryDatabase, ResolvedDependency, SystemInfo, Version};

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo<'a> {
    system_info: &'a SystemInfo,
    r_version: &'a Version,
    dep_info: DependencyInfo<'a>,
    remote_info: RemoteInfo,
}

impl<'a> ProjectInfo<'a> {
    pub fn new(
        lockfile: &'a Option<Lockfile>,
        cache: &'a DiskCache,
        r_version: &'a Version,
        library: &'a Library,
        repository_databases: &'a [(RepositoryDatabase, bool)],
        resolved_dependencies: &'a [ResolvedDependency],
    ) -> Self {
        let dep_info = DependencyInfo::new(
            library.path(),
            lockfile,
            cache,
            library,
            resolved_dependencies,
        );
        let remote_info: RemoteInfo = RemoteInfo::new(&r_version.major_minor(), repository_databases);

        Self {
            system_info: &cache.system_info,
            r_version: r_version,
            dep_info,
            remote_info,
        }
    }
}

impl fmt::Display for ProjectInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "== System Info ==\nOS: {}{}\nR Version: {}\n",
            self.system_info.os_family(),
            self.system_info.arch().map(|a| format!(" ({a})")).unwrap_or_default(),
            self.r_version,
        )?;

        write!(f, "\n== Dependencies ==\n{}", self.dep_info)?;

        write!(f, "\n== Repositories ==\n{}", self.remote_info)?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct DependencyInfo<'a> {
    path: &'a Path,
    total: usize,
    installed: HashMap<&'a Source, (usize, usize)>,
    to_install: HashMap<&'a Source, (usize, usize)>,
    in_cache: HashMap<&'a Source, usize>,
    to_remove: usize,
    non_locked: NonLockedCount,
}

impl<'a> DependencyInfo<'a> {
    pub fn new(
        library_dir: &'a Path,
        lockfile: &'a Option<Lockfile>,
        cache: &'a DiskCache,
        library: &'a Library,
        resolved_dependencies: &'a [ResolvedDependency],
    ) -> Self {
        let mut installed = HashMap::new();
        let mut to_install = HashMap::new();
        let mut in_cache = HashMap::new();
        let mut non_locked = 0usize;

        for r in resolved_dependencies {
            if library.contains_package(&r.name, Some(&r.version)) {
                if let Some(lock) = lockfile {
                    if lock.get_package(&r.name, None).is_some() {
                        update_hash(&mut installed, r);
                    } else {
                        non_locked += 1;
                    }
                } else {
                    update_hash(&mut installed, r);
                }
            } else {
                match cache.get_installation_status(&r.name, &r.version.original, &r.source) {
                    InstallationStatus::Absent => update_hash(&mut to_install, &r),
                    _ => { in_cache.entry(&r.source).and_modify(|v| *v += 1).or_insert(1); },
                }
            }
        };



        // difference between the number of packages in the library and the number of packages installed represents the number of packages to remove
        let to_remove = library.packages.len() - installed.values().map(|(s, b)| s + b).sum::<usize>();

        Self {
            path: library_dir,
            total: resolved_dependencies.len(),
            installed,
            to_install,
            in_cache,
            to_remove,
            non_locked: if lockfile.is_none() {
                NonLockedCount::NoLockfile
            } else {
                NonLockedCount::Lockfile(non_locked)
            }
        }
    }
}

impl fmt::Display for DependencyInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Installed: {}/{}\n{}{}\n", 
            self.installed.values().map(|(s, b)| s + b).sum::<usize>(), 
            self.total,
            if self.to_remove != 0 {
                format!("To remove: {}\n", self.to_remove)
            } else {
                String::new()
            },
            match self.non_locked {
                NonLockedCount::Lockfile(n) if n != 0 => format!("Non-locked packages: {n}\n"),
                _ => String::new(),
            }
        )?;

        write!(f, "Package Sources (Installed/To Install):\n")?;
        Ok(())
    }
}

fn update_hash<'a>(map: &mut HashMap<&'a Source, (usize, usize)>, resolved_dep: &'a ResolvedDependency) {
    if resolved_dep.is_binary() {
        map.entry(&resolved_dep.source).and_modify(|(s, _)| *s += 1).or_insert((1, 0));
    } else {
        map.entry(&resolved_dep.source).and_modify(|(_, b)| *b += 1).or_insert((0, 1));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum NonLockedCount {
    NoLockfile,
    Lockfile(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RemoteInfo {
    repositories: HashMap<String, (usize, usize)>,
}

impl RemoteInfo {
    pub fn new(
        r_version: &[u32; 2],
        repository_databases: &[(RepositoryDatabase, bool)],
    ) -> Self {
        let mut repositories = HashMap::new();
        for (repo_db, force_source )in repository_databases {
            let binary_count = if *force_source { 0 } else { repo_db.get_binary_count(*r_version) };
            let source_count = repo_db.get_source_count();
            repositories.insert(repo_db.url.to_string(), (binary_count, source_count));
        }
        Self {
            repositories
        }
    }
}

impl fmt::Display for RemoteInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (url, (binary_count, source_count)) in self.repositories.iter() {
            write!(f, "{url}: {binary_count} binary packages, {source_count} source_packages\n")?;
        }
        Ok(())
    }
}