use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::Path,
};

use crate::{
    cache::InstallationStatus,
    lockfile::Source,
    package::{Operator, PackageType},
    DiskCache, Library, Lockfile, Repository, RepositoryDatabase, ResolvedDependency, Version,
    VersionRequirement,
};

pub struct DependencyInfo<'a> {
    lib_path: &'a Path,
    dependencies: HashMap<String, Vec<DependencySummary<'a>>>,
    to_remove: HashSet<String>,
    non_locked: HashSet<String>,
}

impl<'a> DependencyInfo<'a> {
    pub fn new(
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
            let dep_sum = DependencySummary::new(r, library, repo_dbs, r_version, cache);
            if !is_in_lock(r.name.as_ref(), lockfile) && dep_sum.status == DependencyStatus::Installed{
                non_locked.insert(r.name.to_string());
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
            "Installed: {}/{}\n{}{}\n",
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
        for (s, dep_vec) in &self.dependencies {
            let counts = Counts::new(dep_vec);
            pkg_source.push_str(&format!(
                "  {}: {}{}{}\n",
                s,
                when_non_zero(
                    &format!("{}/{} binary packages", counts.installed_bin, counts.total_bin),
                    counts.total_bin
                ),
                when_non_zero(", ", (counts.total_bin != 0 && counts.total_src != 0) as usize),
                when_non_zero(
                    &format!("{}/{} source packages", counts.installed_src, counts.total_src),
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
                        when_non_zero(&format!(" ({} to compile)", counts.in_cache_to_compile), counts.in_cache_to_compile)
                    ),
                    counts.in_cache
                ),
                when_non_zero(", ", (counts.in_cache == 0 && counts.to_download == 0) as usize),
                when_non_zero(
                    &format!(
                        "{}/{} to download{}",
                        counts.to_download, 
                        counts.to_install,
                        when_non_zero(&format!(" ({} to compile)", counts.to_download_to_compile), counts.to_download_to_compile)
                    ),
                    counts.to_download
                )
            ));

        }
        write!(f, "{pkg_source}")?;
        write!(f, "{install_summary}")?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
enum DependencyStatus {
    Installed,
    InCache,
    ToCompile,
    Missing,
}

struct DependencySummary<'a> {
    name: &'a str,
    is_binary: bool,
    status: DependencyStatus,
}

impl<'a> DependencySummary<'a> {
    pub fn new(
        resolved_dep: &'a ResolvedDependency,
        library: &Library,
        repo_dbs: &[(RepositoryDatabase, bool)],
        r_version: &Version,
        cache: &DiskCache,
    ) -> Self {
        let is_binary = is_binary_package(resolved_dep, repo_dbs, r_version);

        if library.contains_package(&resolved_dep.name, Some(&resolved_dep.version)) {
            return Self {
                name: &resolved_dep.name,
                is_binary,
                status: DependencyStatus::Installed,
            };
        };

        let status = match cache.get_installation_status(
            &resolved_dep.name,
            &resolved_dep.version.original,
            &resolved_dep.source,
        ) {
            // If the package has a binary in the cache, we can use it independent of if the package is binary or not
            InstallationStatus::Both | InstallationStatus::Binary => DependencyStatus::InCache,
            // If the dependency is not a binary and we have the source in the cache, we can compile it
            InstallationStatus::Source if !is_binary => DependencyStatus::ToCompile,
            // If the dependency is absent or only source when we want a binary, we report it as missing
            _ => DependencyStatus::Missing,
        };

        Self {
            name: &resolved_dep.name,
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
    to_install: usize,
    in_cache: usize,
    in_cache_to_compile: usize,
    to_download: usize,
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
            counts.to_install += 1;
            match dep.status {
                DependencyStatus::InCache => counts.in_cache += 1,
                DependencyStatus::Missing => counts.to_download += 1,
                _ => (),
            }
        }
        counts
    }

    fn install_needed(&self) -> usize {
        self.total_bin + self.total_src - self.installed_bin - self.installed_src
    }
}

fn when_non_zero(s: &str, arg_of_interest: usize) -> &str {
    if arg_of_interest != 0 {
        s
    } else {
        ""
    }
}

// Determine if pkg is in the lockfile, if lockfile is None, we assume all packages are in the lockfile
// This is because we are using if a package is not in a lockfile as a proxy for if it was installed using rv
fn is_in_lock(pkg: &str, lock: Option<&Lockfile>) -> bool {
    lock.map_or(true, |l| l.get_package(pkg, None).is_some())
}

fn is_binary_package(
    resolved_dep: &ResolvedDependency,
    repo_dbs: &[(RepositoryDatabase, bool)],
    r_version: &Version,
) -> bool {
    // We only will say a package is a binary if its from a repository
    let repository = match &resolved_dep.source {
        Source::Repository { repository } => repository,
        _ => return false,
    };
    let ver_req = Some(VersionRequirement::new(
        resolved_dep.version.as_ref().clone(),
        Operator::Equal,
    ));
    repo_dbs
        .iter()
        .find(|(db, _)| &db.url == repository)
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

fn get_dep_id(dep: &ResolvedDependency, repos: &[Repository]) -> String {
    match &dep.source {
        Source::Repository { repository } => repos
            .iter()
            .find(|r| &r.url == repository)
            .map(|r| r.alias.clone())
            .unwrap_or(repository.clone()),
        Source::Git { git, .. } => git.clone(),
        Source::Local { path } => path.to_string_lossy().to_string(),
        Source::Url { url, .. } => url.clone(),
    }
}
