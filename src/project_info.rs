use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    cache::InstallationStatus, lockfile::Source, package::PackageType, DiskCache, Library, Lockfile, Repository, RepositoryDatabase, ResolvedDependency, SystemInfo, Version, VersionRequirement
};

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
        repositories: &'a [Repository],
    ) -> Self {
        let dep_info = DependencyInfo::new(
            library.path(),
            lockfile,
            cache,
            library,
            resolved_dependencies,
            &repository_databases,
            repositories,
            r_version,
        );
        let remote_info: RemoteInfo =
            RemoteInfo::new(&r_version.major_minor(), repository_databases, &repositories);

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
        write!(
            f,
            "== System Info ==\nOS: {}{}\nR Version: {}\n",
            self.system_info.os_family(),
            self.system_info
                .arch()
                .map(|a| format!(" ({a})"))
                .unwrap_or_default(),
            self.r_version,
        )?;

        write!(
            f,
            "\n== Dependencies (Installed/Total) ==\n{}",
            self.dep_info
        )?;

        write!(f, "\n== Repositories ==\n{}", self.remote_info)?;

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct DependencyInfo<'a> {
    path: &'a Path,
    total: usize,
    counts: HashMap<&'a str, (SourceCounts, SourceCounts)>,
    to_remove: usize,
    non_locked: NonLockedCount,
}

#[derive(Debug, Clone, Serialize, Default)]
struct SourceCounts {
    total: usize,
    installed: usize,
    in_cache: usize,
    to_download: usize,
}

impl SourceCounts {
    fn update(&mut self, dep_status: DependencyStatus) {
        match dep_status {
            DependencyStatus::InCache => self.in_cache += 1,
            DependencyStatus::NotInCache => self.to_download += 1,
            DependencyStatus::Installed => {
                self.total += 1;
                self.installed += 1
            }
            DependencyStatus::NotInstalled => self.total += 1,
        }
    }
}

enum DependencyStatus {
    InCache,
    NotInCache,
    Installed,
    NotInstalled,
}

fn update_counts<'a>(
    counts: &mut HashMap<&'a str, (SourceCounts, SourceCounts)>,
    source_id: &'a str,
    dep_status: DependencyStatus,
    is_binary: bool,
) {
    let (source_counts, binary_counts) = counts
        .entry(source_id)
        .or_insert((SourceCounts::default(), SourceCounts::default()));
    if is_binary {
        binary_counts.update(dep_status);
    } else {
        source_counts.update(dep_status);
    }
}

fn is_package_from_binary(
    resolved_dep: &ResolvedDependency,
    repo_dbs: &[(RepositoryDatabase, bool)],
    r_version: &Version,
) -> bool {
    if let Source::Repository { repository } = &resolved_dep.source {
        if let Some((repo_db, _)) = repo_dbs.iter().find(|(db, _)| &db.url == repository) {
            let version_requirement = Some(VersionRequirement::new(
                resolved_dep.version.as_ref().clone(),
                crate::package::Operator::Equal,
            ));
            repo_db
                .find_package(
                    &resolved_dep.name,
                    version_requirement.as_ref(),
                    r_version,
                    resolved_dep.force_source,
                )
                .map(|(_, pkg_type)| pkg_type == PackageType::Binary)
                .unwrap_or(false)
        } else {
            false
        }
    } else {
        false
    }
}

fn get_source_id<'a>(
    resolved_dep: &'a ResolvedDependency,
    repos: &'a [Repository],
) -> &'a str {
    match &resolved_dep.source {
        Source::Repository { repository } => {
            repos.iter().find(|r| r.url() == *repository).map_or("unknown", |r| &r.alias.as_str())
        }
        Source::Local { path } => path.to_str().unwrap_or("local path"),
        Source::Git { git, .. } => git,
        Source::Url { url, .. } => url,
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
        repos: &'a [Repository],
        r_version: &Version,
    ) -> Self {
        let mut counts = HashMap::new();
        let mut non_locked = 0;
        let mut to_remove = 0;
        let mut lib_clone = library
            .packages
            .keys()
            .map(|s| s.to_string())
            .collect::<HashSet<String>>();

        let mut source_id_hash = HashMap::new();

        for r in resolved_dependencies {
            let source_id = source_id_hash.entry(&r.source).or_insert(get_source_id(r, &repos));
            let is_binary = is_package_from_binary(r, repo_dbs, r_version);
            if library.contains_package(&r.name, Some(&r.version)) {
                lib_clone.remove(r.name.as_ref());
                update_counts(&mut counts, &source_id, DependencyStatus::Installed, is_binary);
            } else {
                update_counts(&mut counts, &source_id, DependencyStatus::NotInstalled, is_binary);
                match cache.get_installation_status(&r.name, &r.version.original, &r.source) {
                    // If the package is in the cache as a binary, we want to record it as a binary, no matter if the resolved dependency is from source or binary
                    // This is because we want to use this as a way to convey to the user the performance costs to perform the installation
                    InstallationStatus::Both | InstallationStatus::Binary => {
                        update_counts(&mut counts, &source_id, DependencyStatus::InCache, true)
                    }
                    // We only want to say a source package is in the cache if the resolved dependency is from source.
                    // If the dep is supposed to be from binary, we prefer to download a binary rather than build our own
                    InstallationStatus::Source if !is_binary => {
                        update_counts(&mut counts, &source_id, DependencyStatus::InCache, false)
                    }
                    _ => update_counts(&mut counts, &source_id, DependencyStatus::NotInCache, is_binary),
                }
            }
        }

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
        write!(
            f,
            "Installed: {}/{}\n{}{}\n",
            self.counts
                .values()
                .map(|(s, b)| s.installed + b.installed)
                .sum::<usize>(),
            self.total,
            if self.to_remove != 0 {
                format!("To remove: {}\n", self.to_remove)
            } else {
                String::new()
            },
            match self.non_locked {
                NonLockedCount::Lockfile(n) if n != 0 =>
                    format!("Packages not within lockfile: {n}\n"),
                _ => String::new(),
            }
        )?;

        write!(f, "Package Sources:\n")?;
        let mut install_needed = false;
        for (s, (source_counts, binary_counts)) in self.counts.iter() {
            if source_counts.total != source_counts.installed
                || binary_counts.total != binary_counts.installed
            {
                install_needed = true;
            }
            write!(
                f,
                "  {}: {}{}\n",
                s,
                if binary_counts.total != 0 {
                    format!(
                        "{}/{} binary packages",
                        binary_counts.installed, binary_counts.total
                    )
                } else {
                    String::new()
                },
                if source_counts.total != 0 {
                    format!(
                        "{}{}/{} source packages",
                        if binary_counts.total != 0 { ", " } else { "" },
                        source_counts.installed,
                        source_counts.total
                    )
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
            let diff = binary_counts.total - binary_counts.installed + source_counts.total
                - source_counts.installed;
            let in_cache = binary_counts.in_cache + source_counts.in_cache;
            let to_download = binary_counts.to_download + source_counts.to_download;
            if diff == 0 {
                continue;
            }
            write!(
                f,
                "  {}: {}{}{}\n",
                s,
                if in_cache != 0 {
                    format!(
                        "{in_cache}/{diff} in cache{}",
                        if source_counts.in_cache != 0 {
                            format!(" ({} require compilation)", source_counts.in_cache)
                        } else {
                            String::new()
                        }
                    )
                } else {
                    String::new()
                },
                if in_cache != 0 && to_download != 0 {
                    ", "
                } else {
                    ""
                },
                if to_download != 0 {
                    format!(
                        "{to_download}/{diff} to download{}",
                        if source_counts.to_download != 0 {
                            format!(" ({} require compilation)", source_counts.to_download)
                        } else {
                            String::new()
                        }
                    )
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
    repositories: HashMap<(String, String), (usize, usize)>,
}

fn get_repo_alias<'a>(
    repo_db: &'a RepositoryDatabase,
    repos: &'a [Repository],
) -> &'a str {
    repos.iter().find(|r| r.url() == repo_db.url).map(|r| r.alias.as_str()).unwrap_or("unknown")
}

impl RemoteInfo {
    pub fn new(r_version: &[u32; 2], repository_databases: &[(RepositoryDatabase, bool)], repos: &[Repository]) -> Self {
        let mut repositories = HashMap::new();
        let mut repo_id_hash = HashMap::new();
        for (repo_db, force_source) in repository_databases {
            let repo_id = repo_id_hash.entry(&repo_db.url).or_insert(get_repo_alias(repo_db, repos));
            let binary_count = if *force_source {
                0
            } else {
                repo_db.get_binary_count(*r_version)
            };
            let source_count = repo_db.get_source_count();
            repositories.insert((repo_id.to_string(), repo_db.url.to_string()), (binary_count, source_count));
        }
        Self { repositories }
    }
}

impl fmt::Display for RemoteInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for ((alias, url), (binary_count, source_count)) in self.repositories.iter() {
            write!(
                f,
                "  {alias} ({url}): {binary_count} binary packages, {source_count} source packages\n"
            )?;
        }
        Ok(())
    }
}
