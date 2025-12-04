use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    DiskCache, Library, Lockfile, Repository, RepositoryDatabase, ResolvedDependency, SystemInfo,
    Version, VersionRequirement,
    cache::InstallationStatus,
    lockfile::Source,
    package::{Operator, PackageType},
};
use crate::{
    OsType,
    system_req::{SysDep, SysInstallationStatus},
};
use crate::{repository_urls::get_distro_name, utils::get_max_workers};

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary<'a> {
    r_version: &'a Version,
    system_info: &'a SystemInfo,
    dependency_info: DependencyInfo<'a>,
    cache_root: &'a PathBuf,
    remote_info: RemoteInfo<'a>,
    sys_deps: Vec<SysDep>,
    max_workers: usize,
}

impl<'a> ProjectSummary<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        library: &'a Library,
        resolved_deps: &'a [ResolvedDependency],
        repositories: &'a [Repository],
        repo_dbs: &'a [(RepositoryDatabase, bool)],
        // R version from config to allow for patch specificity in which cache does not allow
        r_version: &'a Version,
        cache: &'a DiskCache,
        lockfile: Option<&'a Lockfile>,
        sys_deps: Vec<SysDep>,
    ) -> Self {
        Self {
            r_version,
            sys_deps,
            system_info: &cache.system_info,
            dependency_info: DependencyInfo::new(
                library,
                resolved_deps,
                repositories,
                repo_dbs,
                r_version,
                cache,
                lockfile,
            ),
            cache_root: &cache.root,
            remote_info: RemoteInfo::new(
                repositories,
                repo_dbs,
                &r_version.major_minor(),
                &cache.system_info,
            ),
            max_workers: get_max_workers(),
        }
    }
}

impl fmt::Display for ProjectSummary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "== System Information == \nOS: {}{}{}\nR Version: {}\n\nNum Workers for Sync: {} ({} cpus available)\nCache Location: {}\n\n",
            self.system_info.os_family(),
            if let OsType::Linux(distro) = self.system_info.os_type {
                format!(" {distro} {}", self.system_info.version)
            } else {
                String::new()
            },
            if let Some(arch) = self.system_info.arch() {
                format!(" ({arch})")
            } else {
                String::new()
            },
            self.r_version,
            self.max_workers,
            num_cpus::get(),
            self.cache_root.as_path().to_string_lossy(),
        )?;

        write!(f, "== Dependencies == \n{}\n", self.dependency_info)?;
        if !self.sys_deps.is_empty() {
            let mut present = 0;
            let mut absent = Vec::new();
            let mut unknown = Vec::new();
            for d in self.sys_deps.iter() {
                match d.status {
                    SysInstallationStatus::Present => present += 1,
                    SysInstallationStatus::Absent => absent.push(d.name.as_str()),
                    SysInstallationStatus::Unknown => unknown.push(d.name.as_str()),
                }
            }

            write!(
                f,
                "== System Dependencies == \n{}{}{}\n",
                if present != 0 {
                    format!("Present: {present}/{}\n", self.sys_deps.len())
                } else {
                    String::new()
                },
                if !absent.is_empty() {
                    format!("Absent:\n  {}\n", absent.join("\n  "))
                } else {
                    String::new()
                },
                if !unknown.is_empty() {
                    format!("Unknown:\n  {}\n", unknown.join("\n  "))
                } else {
                    String::new()
                },
            )?;
        }
        write!(f, "== Remote == \n{}", self.remote_info)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct RepoInfo<'a> {
    url: &'a str,
    binary_count: usize,
    source_count: usize,
}

// TODO: Expand with git + url information
#[derive(Debug, Clone, Serialize)]
struct RemoteInfo<'a> {
    linux_distro_name: Option<LinuxBinaryDistroName>,
    repositories: HashMap<String, RepoInfo<'a>>,
}

#[derive(Debug, Clone, Serialize)]
enum LinuxBinaryDistroName {
    Determined(String),
    Undetermined {
        distro: String,
        version: os_info::Version,
    },
}

impl<'a> RemoteInfo<'a> {
    fn new(
        repos: &'a [Repository],
        repo_dbs: &'a [(RepositoryDatabase, bool)],
        r_version: &[u32; 2],
        system_info: &SystemInfo,
    ) -> Self {
        let mut repositories = HashMap::new();
        for (repo_db, _) in repo_dbs {
            let binary_count = repo_db.get_binary_count(r_version);
            let source_count = repo_db.get_source_count();
            let id = get_repository_alias(&repo_db.url, repos);
            repositories.insert(
                id,
                RepoInfo {
                    url: repo_db.url.as_str(),
                    binary_count,
                    source_count,
                },
            );
        }

        let linux_distro_name = if let OsType::Linux(distro) = system_info.os_type {
            match get_distro_name(system_info, distro) {
                Some(bin_distro_name) => Some(LinuxBinaryDistroName::Determined(bin_distro_name)),
                None => Some(LinuxBinaryDistroName::Undetermined {
                    distro: distro.to_string(),
                    version: system_info.version.clone(),
                }),
            }
        } else {
            None
        };

        Self {
            repositories,
            linux_distro_name,
        }
    }
}

impl fmt::Display for RemoteInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(linux_distro_name) = &self.linux_distro_name {
            match linux_distro_name {
                LinuxBinaryDistroName::Determined(bin_distro_name) => {
                    writeln!(f, "linux binary distribution name: {bin_distro_name}")?
                }
                LinuxBinaryDistroName::Undetermined { distro, version } => writeln!(
                    f,
                    "linux binary distribution name: not available for {} {}",
                    distro, version
                )?,
            }
        }

        let mut repos = self.repositories.iter().collect::<Vec<_>>();
        repos.sort_by_key(|(a, _)| *a);
        for (alias, repo) in repos {
            writeln!(
                f,
                "{alias} ({}): {} binary packages, {} source packages",
                repo.url, repo.binary_count, repo.source_count
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
struct DependencyInfo<'a> {
    lib_path: &'a Path,
    dependencies: HashMap<String, Vec<DependencySummary<'a>>>,
    to_remove: HashSet<String>,
    non_locked: HashSet<String>,
}

impl<'a> DependencyInfo<'a> {
    fn new(
        library: &'a Library,
        resolved_deps: &'a [ResolvedDependency],
        repositories: &'a [Repository],
        repo_dbs: &[(RepositoryDatabase, bool)],
        r_version: &Version,
        cache: &'a DiskCache,
        lockfile: Option<&'a Lockfile>,
    ) -> Self {
        let mut non_locked = HashSet::new();
        let mut to_remove = HashSet::new();
        let mut dependencies: HashMap<String, Vec<DependencySummary>> = HashMap::new();

        // we keep a list of packages within the lib, removing each package as each dependency is processed
        // any libs left in the list either need to be removed or are not locked
        let mut lib_pkgs = library
            .packages
            .keys()
            .map(|s| s.to_string())
            .collect::<HashSet<_>>();

        // we keep track of the dependencies organized by their source identifier
        for r in resolved_deps {
            lib_pkgs.remove(r.name.as_ref());
            let mut dep_sum = DependencySummary::new(r, library, repo_dbs, r_version, cache);
            // if the package was found in the library, but not in the lockfile, we consider it not locked and is missing
            if !is_in_lock(r.name.as_ref(), lockfile)
                && dep_sum.status == DependencyStatus::Installed
            {
                dep_sum.status = DependencyStatus::Missing;
                non_locked.insert(r.name.to_string());
                continue;
            }
            let dep_id = get_dep_id(r, repositories);
            dependencies.entry(dep_id).or_default().push(dep_sum);
        }

        // Any packages still left in lib_pkgs are superfluous and should be removed
        // Additionally, packages that are not in the lockfile need to be reported and additionally removed
        for pkg in &lib_pkgs {
            if is_in_lock(pkg, lockfile) {
                non_locked.insert(pkg.to_string());
            }
            to_remove.insert(pkg.to_string());
        }

        Self {
            lib_path: library.path(),
            dependencies,
            to_remove,
            non_locked,
        }
    }

    fn num_deps_total(&self) -> usize {
        self.dependencies.values().flatten().count()
    }

    fn num_deps_installed(&self) -> usize {
        self.dependencies
            .values()
            .flatten()
            .filter(|d| d.status == DependencyStatus::Installed)
            .count()
    }
}

impl fmt::Display for DependencyInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Library: {}\nInstalled: {}/{}\n{}{}\n",
            self.lib_path.to_string_lossy(),
            self.num_deps_installed(),
            self.num_deps_total(),
            when_non_zero(
                &format!("To remove: {}\n", self.to_remove.len()),
                self.to_remove.len()
            ),
            when_non_zero(
                &format!("Not in lock file: {}\n", self.non_locked.len()),
                self.non_locked.len()
            )
        )?;

        let mut pkg_source = String::from("Package Sources: \n");
        let mut install_summary = String::from("\nInstallation Summary: \n");

        let mut dependencies = self.dependencies.iter().collect::<Vec<_>>();
        dependencies.sort_by_key(|(a, _)| *a);
        for (s, dep_vec) in &self.dependencies {
            let counts = Counts::new(dep_vec);
            pkg_source.push_str(&format!(
                "  {}: {}{}{}\n",
                s,
                when_non_zero(
                    &format!(
                        "{}/{} binary packages",
                        counts.installed_bin, counts.total_bin
                    ),
                    counts.total_bin
                ),
                when_non_zero(
                    ", ",
                    (counts.total_bin != 0 && counts.total_src != 0) as usize
                ),
                when_non_zero(
                    &format!(
                        "{}/{} source packages",
                        counts.installed_src, counts.total_src
                    ),
                    counts.total_src
                ),
            ));
            if counts.to_install == 0 {
                continue;
            }
            install_summary.push_str(&format!(
                "  {}: {}{}{}\n",
                s,
                when_non_zero(
                    &format!(
                        "{}/{} in cache{}",
                        counts.in_cache,
                        counts.to_install,
                        when_non_zero(
                            &format!(" ({} to compile)", counts.in_cache_to_compile),
                            counts.in_cache_to_compile
                        )
                    ),
                    counts.in_cache
                ),
                when_non_zero(
                    ", ",
                    (counts.in_cache != 0 && counts.to_download != 0) as usize
                ),
                when_non_zero(
                    &format!(
                        "{}/{} to download{}",
                        counts.to_download,
                        counts.to_install,
                        when_non_zero(
                            &format!(" ({} to compile)", counts.to_download_to_compile),
                            counts.to_download_to_compile
                        )
                    ),
                    counts.to_download
                )
            ));
        }
        write!(f, "{pkg_source}")?;
        // If there are no packages to install from any of the sources, install_summary will never be edited and there is no reason to print
        if install_summary != *"\nInstallation Summary: \n" {
            write!(f, "{install_summary}")?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
enum DependencyStatus {
    // If the package is in the library
    Installed,
    // If the package has a binary in the cache
    InCache,
    // If a src package has a src in the cache (but not a binary)
    ToCompile,
    // If a package is not in the cache
    Missing,
}

#[derive(Debug, Clone, Serialize)]
struct DependencySummary<'a> {
    #[serde(rename = "name")]
    _name: &'a str, // for eventual ability to list pkg names
    is_binary: bool,
    status: DependencyStatus,
}

// TODO: implement custom Serialize for this
impl<'a> DependencySummary<'a> {
    pub fn new(
        resolved_dep: &'a ResolvedDependency,
        library: &Library,
        repo_dbs: &[(RepositoryDatabase, bool)],
        r_version: &Version,
        cache: &DiskCache,
    ) -> Self {
        // determine if the dependency can come as a binary
        let is_binary = is_binary_package(resolved_dep, repo_dbs, r_version);

        // If the package is within the library, immediately return saying it is installed
        if library.contains_package(resolved_dep) {
            return Self {
                _name: &resolved_dep.name,
                is_binary,
                status: DependencyStatus::Installed,
            };
        };

        // If the package is resolved as builtin, then we consider it installed
        let status = match cache.get_installation_status(
            &resolved_dep.name,
            &resolved_dep.version.original,
            &resolved_dep.source,
        ) {
            // If the package has a binary in the cache, we can use it independent of if the package is binary or not
            InstallationStatus::Both(_) | InstallationStatus::Binary(_) => {
                DependencyStatus::InCache
            }
            // If the dependency is not a binary and we have the source in the cache, we can compile it
            InstallationStatus::Source if !is_binary => DependencyStatus::ToCompile,
            // If the dependency is absent or only source when we want a binary, we report it as missing
            _ => DependencyStatus::Missing,
        };

        Self {
            _name: &resolved_dep.name,
            is_binary,
            status,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Counts {
    total_bin: usize,
    total_src: usize,
    installed_bin: usize,
    installed_src: usize,
    to_install: usize, // total - installed (for binaries and src)
    in_cache: usize,   // all packages in cache, even if compilation required
    in_cache_to_compile: usize,
    to_download: usize, // all packages to download, even if compilation required
    to_download_to_compile: usize,
}

impl Counts {
    fn new(dep_vec: &[DependencySummary]) -> Self {
        let mut counts = Counts {
            total_bin: 0,
            total_src: 0,
            installed_bin: 0,
            installed_src: 0,
            to_install: 0,
            in_cache: 0,
            in_cache_to_compile: 0,
            to_download: 0,
            to_download_to_compile: 0,
        };

        for dep in dep_vec {
            // Some fields on Counts are dependent on if the dep is from binary or not
            if dep.is_binary {
                counts.total_bin += 1;
                if let DependencyStatus::Installed = dep.status {
                    counts.installed_bin += 1;
                    continue;
                }
            } else {
                counts.total_src += 1;
                match dep.status {
                    DependencyStatus::Installed => {
                        counts.installed_src += 1;
                        continue;
                    }
                    DependencyStatus::ToCompile => counts.in_cache_to_compile += 1,
                    DependencyStatus::Missing => counts.to_download_to_compile += 1,
                    _ => (),
                }
            }
            // Other fields are updated agnostic to if the dep is from binary or not
            counts.to_install += 1;
            match dep.status {
                DependencyStatus::InCache | DependencyStatus::ToCompile => counts.in_cache += 1,
                DependencyStatus::Missing => counts.to_download += 1,
                _ => (),
            }
        }
        counts
    }
}

fn when_non_zero(s: &str, arg_of_interest: usize) -> &str {
    if arg_of_interest != 0 { s } else { "" }
}

// Determine if pkg is in the lockfile, if lockfile is None, we assume all packages are in the lockfile
// This is because we are using if a package is not in a lockfile as a proxy for if it was installed using rv
fn is_in_lock(pkg: &str, lock: Option<&Lockfile>) -> bool {
    lock.is_none_or(|l| l.get_package(pkg, None).is_some())
}

fn is_binary_package(
    resolved_dep: &ResolvedDependency,
    repo_dbs: &[(RepositoryDatabase, bool)],
    r_version: &Version,
) -> bool {
    // We only will say a package is a binary if its from a repository or its built in
    let repository = match &resolved_dep.source {
        Source::Repository { repository } => repository,
        Source::Builtin { .. } => return true,
        _ => return false,
    };
    let ver_req = Some(VersionRequirement::new(
        resolved_dep.version.as_ref().clone(),
        Operator::Equal,
    ));
    repo_dbs
        .iter()
        .find(|(db, _)| db.url == repository.as_str())
        .and_then(|(db, _)| {
            db.find_package(
                &resolved_dep.name,
                ver_req.as_ref(),
                r_version,
                resolved_dep.force_source,
            )
        })
        .map(|(_, pkg)| pkg == PackageType::Binary)
        .unwrap_or(false)
}

// dependency sources are ID'd by the following
// - repositories: alias in the config. Defaults to the url if no alias found
// - git: repository url
// - local: path to local package
// - url: package url
// - builtin: "builtin"
fn get_dep_id(dep: &ResolvedDependency, repos: &[Repository]) -> String {
    match &dep.source {
        Source::Repository { repository } => get_repository_alias(repository.as_str(), repos),
        Source::RUniverse { repository, .. } => get_repository_alias(repository.as_str(), repos),
        Source::Git { git, .. } => git.to_string(),
        Source::Local { path, .. } => path.to_string_lossy().to_string(),
        Source::Url { url, .. } => url.to_string(),
        Source::Builtin { .. } => "builtin".to_string(),
    }
}

fn get_repository_alias(r: &str, repos: &[Repository]) -> String {
    repos
        .iter()
        .find(|repo| repo.url() == r)
        .map(|repo| repo.alias.to_string())
        .unwrap_or(r.to_string())
}
