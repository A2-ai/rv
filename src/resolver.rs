use crate::Cache;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::cache::InstallationStatus;
use crate::config::DependencyKind;
use crate::lockfile::Lockfile;
use crate::package::PackageType;
use crate::repository::RepositoryDatabase;
use crate::version::{Version, VersionRequirement};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ResolvedDependency<'d> {
    pub(crate) name: &'d str,
    pub(crate) version: &'d str,
    pub(crate) repository_url: &'d str,
    pub(crate) dependencies: Vec<&'d str>,
    pub(crate) force_source: bool,
    pub(crate) kind: PackageType,
    pub(crate) installation_status: InstallationStatus,
    pub(crate) path: Option<&'d str>,
    pub(crate) found_in_lockfile: bool,
}

impl<'d> ResolvedDependency<'d> {
    pub fn is_installed(&self) -> bool {
        match self.kind {
            PackageType::Source => self.installation_status.source_available(),
            PackageType::Binary => self.installation_status.binary_available(),
        }
    }
}

impl<'a> fmt::Display for ResolvedDependency<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}={} (from {}, type={}, from_lockfile={}, path='{}')",
            self.name, self.version, self.repository_url, self.kind, self.found_in_lockfile, self.path.unwrap_or("")
        )
    }
}

#[derive(Debug, PartialEq, Clone)]
enum UnresolvedDependencyKind<'d> {
    /// The user provided a dependency that doesn't exist
    Direct,
    /// A package has a dependency not found. It could be nested several times,
    /// we only show the immediate parent which could be an indirect dep as well.
    Indirect(&'d str),
}

#[derive(Debug, PartialEq, Clone)]
pub struct UnresolvedDependency<'d> {
    name: &'d str,
    version_requirement: Option<&'d VersionRequirement>,
    origins: Vec<UnresolvedDependencyKind<'d>>,
}

impl<'a> fmt::Display for UnresolvedDependency<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut origins = Vec::with_capacity(self.origins.len());
        for origin in &self.origins {
            let v = match origin {
                UnresolvedDependencyKind::Direct => "user provided".to_string(),
                UnresolvedDependencyKind::Indirect(parent) => format!("dependency of `{parent}`"),
            };
            origins.push(v);
        }

        write!(
            f,
            "{}{} {}",
            self.name,
            if let Some(l) = self.version_requirement {
                format!(" {l} ")
            } else {
                String::new()
            },
            format!("from: {}", origins.join(", "))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionNeeded {
    /// Nothing to do
    None,
    /// We only need to remove those deps from the lockfile + project library
    RemoveOnly(Vec<String>),
    /// We will need to look up the package databases so do a full lookup, preferring
    /// versions already in the lockfile
    Full,
}

#[derive(Debug, PartialEq)]
pub struct Resolver<'d> {
    /// The repositories are stored in the order defined in the config
    /// The last should get priority over previous repositories
    /// If the bool is `true`, this means this repository should only look at sources
    repositories: &'d [(RepositoryDatabase, bool)],
    r_version: &'d Version,
    /// If we have a lockfile for the resolver, we will skip looking at the database for any package
    /// listed in it
    lockfile: Option<&'d Lockfile>,
}

impl<'d> Resolver<'d> {
    pub fn new(
        repositories: &'d [(RepositoryDatabase, bool)],
        r_version: &'d Version,
        lockfile: Option<&'d Lockfile>,
    ) -> Self {
        Self {
            repositories,
            r_version,
            lockfile,
        }
    }

    pub fn set_repositories(&mut self, repositories: &'d [(RepositoryDatabase, bool)]) {
        self.repositories = repositories;
    }

    /// Tries to find all dependencies from the repos, as well as their install status
    pub fn resolve(
        &self,
        dependencies: &'d [DependencyKind],
        cache: &'d impl Cache,
    ) -> (Vec<ResolvedDependency<'d>>, Vec<UnresolvedDependency<'d>>) {
        let mut resolved = Vec::new();
        // We might have the same unresolved dep multiple times.
        let mut unresolved = HashMap::<&str, UnresolvedDependency>::new();
        let mut found = HashSet::with_capacity(dependencies.len() * 10);

        let mut queue: VecDeque<_> = dependencies
            .iter()
            .map(|d| {
                (
                    d.name(),
                    d.repository(),
                    // required version
                    None,
                    d.install_suggestions(),
                    d.force_source(),
                    None,
                )
            })
            .collect();

        while let Some((
            name,
            repository,
            version_requirement,
            install_suggestions,
            pkg_force_source,
            parent,
        )) = queue.pop_front()
        {
            // If we have already found that dependency, skip it
            // TODO: maybe different version req? we can cross that bridge later
            if found.contains(name) {
                continue;
            }

            // Look at lockfile before looking up any repositories
            if let Some(lockfile) = self.lockfile {
                // If we found the package in the lockfile, consider it found and do not look up
                // the repo at all
                if let Some(package) = lockfile.get_package(name, pkg_force_source) {
                    found.insert(name);
                    resolved.push(ResolvedDependency {
                        name: &package.name,
                        version: &package.version,
                        repository_url: package.repo_url().unwrap(),
                        dependencies: package.dependencies.iter().map(|d| d.as_str()).collect(),
                        kind: if pkg_force_source {
                            PackageType::Source
                        } else {
                            PackageType::Binary
                        },
                        force_source: pkg_force_source,
                        installation_status: cache.get_package_installation_status(
                            package.repo_url().unwrap(),
                            &package.name,
                            &package.version,
                        ),
                        path: package.path.as_ref().map(|x| x.as_str()),
                        found_in_lockfile: true,
                    });

                    for d in &package.dependencies {
                        if !found.contains(d.as_str()) {
                            queue.push_back((d.as_str(), None, None, false, false, Some(name)));
                        }
                    }

                    continue;
                }
            }

            for (repo, repo_source_only) in self.repositories {
                if let Some(r) = repository {
                    if repo.name != r {
                        continue;
                    }
                }

                if let Some((package, package_type)) = repo.find_package(
                    name,
                    version_requirement,
                    self.r_version,
                    pkg_force_source || *repo_source_only,
                ) {
                    found.insert(name);
                    let deps = package.dependencies_to_install(install_suggestions);
                    let all_dependencies = deps.direct;
                  
                    resolved.push(ResolvedDependency {
                        name: &package.name,
                        version: &package.version.original,
                        repository_url: &repo.url,
                        dependencies: all_dependencies.iter().map(|d| d.name()).collect(),
                        kind: package_type,
                        force_source: pkg_force_source || *repo_source_only,
                        installation_status: cache.get_package_installation_status(
                            &repo.url,
                            &package.name,
                            &package.version.original,
                        ),
                        path: package.path.as_ref().map(|x| x.as_str()),
                        found_in_lockfile: false,
                    });

                    for d in all_dependencies {
                        if !found.contains(d.name()) {
                            queue.push_back((
                                d.name(),
                                None,
                                d.version_requirement(),
                                false,
                                false,
                                Some(name),
                            ));
                        }
                    }
                    if install_suggestions {
                        // given all the explicitly called out deps are already in the queue
                        // we don't need to worry about if this is a copy of one of those values
                        // since they'll be in front of the queue and will get found/setup for resolution first
                        // since they're suggested deps we also don't want their suggests for now either,
                        // though one day we might need to add a recursive suggests akin to deps = TRUE
                        // for R. This would cover that situation a bit
                        for d in deps.suggests.unwrap() {
                            if !found.contains(d.name()) {
                                queue.push_back((
                                    d.name(),
                                    None,
                                    d.version_requirement(),
                                    false,
                                    false,
                                    Some(name),
                                ));
                            }
                        }
                    } 
                    break;
                }
            }

            if !found.contains(name) {
                let ud_kind = if let Some(p) = parent {
                    UnresolvedDependencyKind::Indirect(p)
                } else {
                    UnresolvedDependencyKind::Direct
                };
                if let Some(ud) = unresolved.get_mut(name) {
                    ud.origins.push(ud_kind);
                } else {
                    unresolved.insert(
                        name,
                        UnresolvedDependency {
                            name,
                            version_requirement,
                            origins: vec![ud_kind],
                        },
                    );
                }
            }
        }

        (resolved, unresolved.into_values().collect())
    }

    // TODO: add tests
    pub fn resolution_needed(&self, dependencies: &'d [DependencyKind]) -> ResolutionNeeded {
        // If we don't have a lockfile, we'll need to lookup everything
        if self.lockfile.is_none() {
            return ResolutionNeeded::Full;
        }
        let lockfile = self.lockfile.unwrap();
        // At this point we need to figure out 2 things:
        // 1. whether we have all the explicit deps and their deps in the lockfile
        // 2. whether we removed some dep in the config file and need to update the lockfile
        //    and just remove those from the lockfile/rv dir
        let lockfile_deps = lockfile.package_names();
        let mut deps_seen: HashSet<&str> = HashSet::new();

        for d in dependencies {
            // TODO: add source (repository url/git) to the param if set since changing that means a new package
            let all_deps = lockfile.get_package_tree(d.name(), d.force_source());
            // If we don't have an explicit dep in the lockfile, we'll need a full resolve
            if all_deps.is_empty() {
                return ResolutionNeeded::Full;
            }
            deps_seen.extend(all_deps.into_iter());
        }

        // Check whether we have things we need to remove or not
        let unneeded_deps = lockfile_deps
            .difference(&deps_seen)
            .map(|d| d.to_string())
            .collect::<Vec<_>>();

        if unneeded_deps.is_empty() {
            ResolutionNeeded::None
        } else {
            ResolutionNeeded::RemoveOnly(unneeded_deps)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::r_cmd::{InstallError, VersionError};
    use crate::repository::RepositoryDatabase;
    use crate::{CacheEntry, RCmd};
    use serde::Deserialize;
    use std::io::Error;
    use std::path::{Path, PathBuf};
    use std::str::FromStr;

    struct FakeCache;

    impl Cache for FakeCache {
        fn get_package_db_entry(&self, _: &str) -> CacheEntry {
            CacheEntry::NotFound(PathBuf::from_str("").unwrap())
        }

        fn get_package_installation_status(&self, _: &str, _: &str, _: &str) -> InstallationStatus {
            InstallationStatus::Absent
        }
    }

    struct FakeRCmd;
    impl RCmd for FakeRCmd {
        fn install(
            &self,
            _: impl AsRef<Path>,
            _: impl AsRef<Path>,
            _: impl AsRef<Path>,
        ) -> Result<String, InstallError> {
            todo!()
        }

        fn check(
            &self,
            _: &Path,
            _: &Path,
            _: Vec<&str>,
            _: Vec<(&str, &str)>,
        ) -> Result<(), Error> {
            todo!()
        }

        fn build(
            &self,
            _: &Path,
            _: &Path,
            _: &Path,
            _: Vec<&str>,
            _: Vec<(&str, &str)>,
        ) -> Result<(), Error> {
            todo!()
        }

        fn version(&self) -> Result<Version, VersionError> {
            todo!()
        }
    }

    #[derive(Debug, Deserialize)]
    struct TestRepo {
        name: String,
        source: Option<String>,
        binary: Option<String>,
        force_source: bool,
    }

    #[derive(Debug, Deserialize)]
    struct TestRepositories {
        repos: Vec<TestRepo>,
    }

    fn extract_test_elements(
        path: &Path,
    ) -> (Config, Version, Vec<(RepositoryDatabase, bool)>, Lockfile) {
        let content = std::fs::read_to_string(path).unwrap();
        let parts: Vec<_> = content.splitn(3, "---").collect();
        let config = Config::from_str(parts[0]).expect("valid config");
        let r_version = config.get_r_version(FakeRCmd {}).unwrap();
        let repositories = if let Ok(data) = toml::from_str::<TestRepositories>(parts[1]) {
            let mut res = Vec::new();
            for r in data.repos {
                let mut repo = RepositoryDatabase::new(&r.name, &format!("http://{}", r.name));
                if let Some(p) = r.source {
                    let path = format!("src/tests/package_files/{p}.PACKAGE");
                    let text = std::fs::read_to_string(&path).unwrap();
                    repo.parse_source(&text);
                }

                if let Some(p) = r.binary {
                    let path = format!("src/tests/package_files/{p}.PACKAGE");
                    let text = std::fs::read_to_string(&path).unwrap();
                    repo.parse_binary(&text, r_version.major_minor());
                }
                res.push((repo, r.force_source));
            }
            res
        } else {
            let mut repo = RepositoryDatabase::new("inline", "");
            repo.parse_source(parts[1]);
            vec![(repo, false)]
        };
        let lockfile = if parts[2].is_empty() {
            Lockfile::new(&r_version.original)
        } else {
            Lockfile::from_str(parts[2]).expect("valid lockfile")
        };

        (config, r_version, repositories, lockfile)
    }

    #[test]
    fn resolving() {
        let paths = std::fs::read_dir("src/tests/resolution/").unwrap();
        for path in paths {
            let p = path.unwrap().path();
            let (config, r_version, repositories, lockfile) = extract_test_elements(&p);
            let resolver = Resolver::new(&repositories, &r_version, Some(&lockfile));
            let (resolved, unresolved) = resolver.resolve(&config.dependencies(), &FakeCache {});
            // let new_lockfile = Lockfile::from_resolved(&r_version.major_minor(), &resolved);
            // println!("{}", new_lockfile.as_toml_string());
            let mut out = String::new();
            for d in resolved {
                out.push_str(&d.to_string());
                out.push_str("\n");
            }

            if !unresolved.is_empty() {
                out.push_str("--- unresolved --- \n");
                for d in unresolved {
                    out.push_str(&d.to_string());
                    out.push_str("\n");
                }
            }
            // Output has been compared with pkgr for the same PACKAGE file
            insta::assert_snapshot!(p.file_name().unwrap().to_string_lossy().to_string(), out);
        }
    }
}
