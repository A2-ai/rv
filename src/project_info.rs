use std::{collections::{HashMap, HashSet}, fmt, path::Path};

use serde::{Serialize, Deserialize};

use crate::{cache::InstallationStatus, lockfile::Source, package::PackageType, DiskCache, Library, Lockfile, RepositoryDatabase, ResolvedDependency, SystemInfo, Version, VersionRequirement};

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
            &repository_databases,
            r_version,
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

        write!(f, "\n== Dependencies (Installed/Total)==\n{}", self.dep_info)?;

        write!(f, "\n== Repositories ==\n{}", self.remote_info)?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct DependencyInfo<'a> {
    path: &'a Path,
    total: usize,
    counts: HashMap<&'a Source, (SourceCounts, SourceCounts)>, 
    to_remove: usize,
    non_locked: NonLockedCount,
}

#[derive(Debug, Clone, Serialize, Default)]
struct SourceCounts {
    total: usize,
    installed: usize,
    in_cache: usize,
}

impl SourceCounts {
    fn update(&mut self, dep_status: DependencyStatus) {
        match dep_status {
            DependencyStatus::InCache => {
                self.in_cache += 1;
                self.total += 1;
            },
            DependencyStatus::Installed => {
                self.total += 1;
                self.installed += 1
            },
            DependencyStatus::NotInstalled => self.total += 1,
        }
    }
}

enum DependencyStatus {
    InCache,
    Installed,
    NotInstalled,
}

fn update_counts<'a>(
    counts: &mut HashMap<&'a Source, (SourceCounts, SourceCounts)>,
    resolved_dep: &'a ResolvedDependency,
    dep_status: DependencyStatus,
    is_binary: bool
) {
    let (source_counts, binary_counts) = counts.entry(&resolved_dep.source).or_insert((SourceCounts::default(), SourceCounts::default()));
    if is_binary {
        binary_counts.update(dep_status);
    } else {
        source_counts.update(dep_status);
    }
}

fn is_package_from_binary(resolved_dep: &ResolvedDependency, repo_dbs: &[(RepositoryDatabase, bool)], r_version: &Version) -> bool {
    if let Source::Repository { repository } = &resolved_dep.source {
        if let Some((repo_db, force_source)) = repo_dbs.iter().find(|(db, _)| &db.url == repository) {
            let version_requirement = Some(VersionRequirement::new(resolved_dep.version.as_ref().clone(), crate::package::Operator::Equal));
            repo_db.find_package(&resolved_dep.name, version_requirement.as_ref(), r_version, *force_source)
                .map(|(_, pkg_type)| pkg_type == PackageType::Binary)
                .unwrap_or(false)
        } else {
            false
        }
    } else {
        false
    }
}

impl<'a> DependencyInfo<'a> {
    pub fn new(
        library_dir: &'a Path,
        lockfile: &'a Option<Lockfile>,
        cache: &'a DiskCache,
        library: &'a Library,
        resolved_dependencies: &'a [ResolvedDependency],
        repo_dbs: &[(RepositoryDatabase, bool)],
        r_version: &Version,
    ) -> Self {
        let mut counts = HashMap::new();
        let mut non_locked = 0;
        let mut to_remove = 0;
        let mut lib_clone = library.packages.keys().map(|s| s.to_string()).collect::<HashSet<String>>();

        for r in resolved_dependencies {
            let is_binary = is_package_from_binary(r, repo_dbs, r_version);
            if library.contains_package(&r.name, Some(&r.version)) {
                lib_clone.remove(r.name.as_ref());
                update_counts(&mut counts, &r, DependencyStatus::Installed, is_binary);
            } else {
                match cache.get_installation_status(&r.name, &r.version.original, &r.source) {
                    // If the package is in the cache as a binary, we want to record it as a binary, no matter if the resolved dependency is from source or binary
                    // This is because we want to use this as a way to convey to the user the performance costs to perform the installation
                    InstallationStatus::Both | InstallationStatus::Binary => update_counts(&mut counts, &r, DependencyStatus::InCache, true),
                    // We only want to say a source package is in the cache if the resolved dependency is from source.
                    // If the dep is supposed to be from binary, we prefer to download a binary rather than build our own
                    InstallationStatus::Source if !is_binary => update_counts(&mut counts, &r, DependencyStatus::InCache, false),
                    _ => update_counts(&mut counts, &r, DependencyStatus::NotInstalled, is_binary),
                }
            }
        };

        for name in lib_clone {
            if let Some(lock) = lockfile {
                if lock.get_package(&name, None).is_some() {
                    to_remove += 1;
                } else {
                    non_locked += 1;
                }
            } else {
                to_remove += 1;
            }
        }

        let non_locked = if lockfile.is_none() {
            NonLockedCount::NoLockfile
        } else {
            NonLockedCount::Lockfile(non_locked)
        };

        Self {
            path: library_dir,
            total: resolved_dependencies.len(),
            counts,
            to_remove,
            non_locked,
        }
    }
}

impl fmt::Display for DependencyInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Installed: {}/{}\n{}{}\n", 
            self.counts.values().map(|(s, b)| s.installed + b.installed).sum::<usize>(), 
            self.total,
            if self.to_remove != 0 {
                format!("To remove: {}\n", self.to_remove)
            } else {
                String::new()
            },
            match self.non_locked {
                NonLockedCount::Lockfile(n) if n != 0 => format!("Packages not within lockfile: {n}\n"),
                _ => String::new(),
            }
        )?;

        write!(f, "Package Sources:\n")?;
        let mut install_needed = false;
        for (s, (source_counts, binary_counts)) in self.counts.iter() {
            if source_counts.total != source_counts.installed && binary_counts.total != binary_counts.installed {
                install_needed = true;
            }
            write!(f, "  {}: {}{}\n", s, 
                if binary_counts.total != 0 {
                    format!("{}/{} binary packages", binary_counts.installed, binary_counts.total)
                } else {
                    String::new()
                },
                if source_counts.total != 0 {
                    format!("{}{}/{} source packages", if binary_counts.total != 0 { ", "} else { "" }, source_counts.installed, source_counts.total)
                } else {
                    String::new()
                }
            )?;
        }

        if !install_needed {
            return Ok(());
        }
        write!(f, "\nInstallation Summary:\n")?;
        for (s, (source_counts, binary_counts)) in self.counts.iter() {
            let binary_diff = binary_counts.total - binary_counts.installed;
            let source_diff = source_counts.total - source_counts.installed;
            if binary_diff == 0 && source_diff == 0 {
                continue;
            }
            write!(f, "  {}: {}{} present in cache\n", s,
                if binary_diff != 0 {
                    format!("{}/{} binary packages", binary_counts.in_cache, binary_diff)
                } else {
                    String::new()
                },
                if source_counts.total != 0 {
                    format!("{}{}/{} source packages", if binary_diff != 0 { ", "} else { "" }, source_counts.in_cache, source_diff)
                } else {
                    String::new()
                }
            )?;
        }


        Ok(())
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