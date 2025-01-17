use crate::Cache;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

use crate::cache::InstallationStatus;
use crate::config::DependencyKind;
use crate::package::PackageType;
use crate::repository::RepositoryDatabase;
use crate::version::{Version, VersionRequirement};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ResolvedDependency<'d> {
    pub(crate) name: &'d str,
    pub(crate) version: &'d str,
    /// Repository alias in the config
    pub(crate) repository: &'d str,
    pub(crate) repository_url: &'d str,
    pub(crate) dependencies: Vec<&'d str>,
    pub(crate) needs_compilation: bool,
    pub(crate) kind: PackageType,
    pub(crate) installation_status: InstallationStatus,
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
            "{}={} (from {}, type={}, status={})",
            self.name, self.version, self.repository, self.kind, self.installation_status,
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
            "{}{}{}",
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

#[derive(Debug, PartialEq)]
pub struct Resolver<'d> {
    /// The repositories are stored in the order defined in the config
    /// The last should get priority over previous repositories
    /// If the bool is `true`, this means this repository should only look at sources
    repositories: &'d [(RepositoryDatabase, bool)],
    r_version: &'d Version,
}

impl<'d> Resolver<'d> {
    pub fn new(repositories: &'d [(RepositoryDatabase, bool)], r_version: &'d Version) -> Self {
        Self {
            repositories,
            r_version,
        }
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
                    let (all_dependencies, maybe_suggests) = package.dependencies_to_install(install_suggestions);
                    if install_suggestions {
                        // given all the explicitly called out deps are already in the queue
                        // we don't need to worry about if this is a copy of one of those values
                        // since they'll be in front of the queue and will get found/setup for resolution first
                        // since they're suggested deps we also don't want their suggests for now either,
                        // though one day we might need to add a recursive suggests akin to deps = TRUE
                        // for R. This would cover that situation a bit
                        let addl_deps: Vec<_> = maybe_suggests
                            .into_iter()
                            .map(|d| {
                                // these values  
                                (
                                    d.name(),
                                    // up for consideration if this dep should also inherit the repo preference of the parent
                                    // but for now we're going to say no and just let it resolve naturally
                                    None::<&str>,
                                    d.version_requirement(),
                                    false,
                                    false,
                                    Some(package.name.as_ref()),
                                )
                            })
                            .collect();
                        queue.extend(addl_deps);
                    }
                    resolved.push(ResolvedDependency {
                        name: &package.name,
                        version: &package.version.original,
                        repository: &repo.name,
                        repository_url: &repo.url,
                        dependencies: all_dependencies.iter().map(|d| d.name()).collect(),
                        needs_compilation: package.needs_compilation,
                        kind: package_type,
                        installation_status: cache.get_package_installation_status(
                            &repo.url,
                            &package.name,
                            &package.version.original,
                        ),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::repository::RepositoryDatabase;
    use crate::CacheEntry;
    use std::path::PathBuf;
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

    #[test]
    fn can_resolve_various_dependencies() {
        let paths = std::fs::read_dir("src/tests/resolution/").unwrap();
        let mut repositories = Vec::new();
        let r_version = Version::from_str("4.4.2").unwrap();

        for (name, (src_filename, binary_filename)) in vec![
            ("gh-mirror", ("gh-pkg-mirror.PACKAGE", None)),
            ("test", ("posit-src.PACKAGE", Some("cran-binary.PACKAGE"))),
        ] {
            let content =
                std::fs::read_to_string(format!("src/tests/package_files/{src_filename}")).unwrap();
            let mut repository = RepositoryDatabase::new(name, "");
            repository.parse_source(&content);
            if let Some(bin) = binary_filename {
                let content =
                    std::fs::read_to_string(format!("src/tests/package_files/{bin}")).unwrap();
                repository.parse_binary(&content, r_version.major_minor());
            }
            repositories.push((repository, false));
        }

        for path in paths {
            let p = path.unwrap().path();
            let config = Config::from_file(&p).unwrap();
            let v = if p.file_name().unwrap() == "higher_r_version.toml" {
                Version::from_str("4.5").unwrap()
            } else {
                r_version.clone()
            };
            let resolver = Resolver::new(&repositories, &v);
            let (resolved, unresolved) = resolver.resolve(&config.dependencies(), &FakeCache {});
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
