use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::{fmt, fs};

use crate::cache::InstallationStatus;
use crate::config::DependencyKind;
use crate::consts::DESCRIPTION_FILENAME;
use crate::git::GitReference;
use crate::lockfile::{LockedPackage, Lockfile, Source};
use crate::package::{parse_description_file, InstallationDependencies, Package, PackageType};
use crate::repository::RepositoryDatabase;
use crate::version::{Version, VersionRequirement};
use crate::{Cache, GitOperations};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ResolvedDependency<'d> {
    pub(crate) name: Cow<'d, str>,
    pub(crate) version: Cow<'d, str>,
    pub(crate) source: Source,
    pub(crate) dependencies: Vec<Cow<'d, str>>,
    pub(crate) suggests: Vec<Cow<'d, str>>,
    pub(crate) force_source: bool,
    pub(crate) install_suggests: bool,
    pub(crate) kind: PackageType,
    pub(crate) installation_status: InstallationStatus,
    pub(crate) path: Option<Cow<'d, str>>,
    pub(crate) found_in_lockfile: bool,
}

impl<'d> ResolvedDependency<'d> {
    pub fn is_installed(&self) -> bool {
        match self.kind {
            PackageType::Source => self.installation_status.source_available(),
            PackageType::Binary => self.installation_status.binary_available(),
        }
    }

    pub fn from_locked_package(package: &'d LockedPackage, cache: &'d impl Cache) -> Self {
        Self {
            name: Cow::Borrowed(&package.name),
            version: Cow::Borrowed(&package.version),
            source: package.source.clone(),
            dependencies: package
                .dependencies
                .iter()
                .map(|d| Cow::Borrowed(d.as_str()))
                .collect(),
            suggests: package
                .suggests
                .iter()
                .map(|s| Cow::Borrowed(s.as_str()))
                .collect(),
            // TODO: what should we do here?
            kind: if package.force_source {
                PackageType::Source
            } else {
                PackageType::Binary
            },
            force_source: package.force_source,
            install_suggests: package.install_suggests(),
            installation_status: cache.get_package_installation_status(
                package.source.repository_url(),
                &package.name,
                &package.version,
            ),
            path: package.path.as_ref().map(|x| Cow::Borrowed(x.as_str())),
            found_in_lockfile: true,
        }
    }

    // TODO: 2 bool not great but maybe ok if it's only used in one place
    pub fn from_package_repository(
        package: &'d Package,
        repo_url: &str,
        package_type: PackageType,
        install_suggestions: bool,
        force_source: bool,
        cache: &'d impl Cache,
    ) -> (Self, InstallationDependencies<'d>) {
        let deps = package.dependencies_to_install(install_suggestions);

        let res = ResolvedDependency {
            name: Cow::Borrowed(&package.name),
            version: Cow::Borrowed(&package.version.original),
            source: Source::Repository {
                repository: repo_url.to_string(),
            },
            dependencies: deps
                .direct
                .iter()
                .map(|d| Cow::Borrowed(d.name()))
                .collect(),
            suggests: deps
                .suggests
                .iter()
                .map(|d| Cow::Borrowed(d.name()))
                .collect(),
            kind: package_type,
            force_source,
            install_suggests: install_suggestions,
            installation_status: cache.get_package_installation_status(
                repo_url,
                &package.name,
                &package.version.original,
            ),
            path: package.path.as_ref().map(|x| Cow::Borrowed(x.as_str())),
            found_in_lockfile: false,
        };

        (res, deps)
    }

    pub fn from_git_package<'p>(
        package: &'p Package,
        repo_url: &str,
        sha: String,
        install_suggestions: bool,
        cache: &'d impl Cache,
    ) -> (Self, InstallationDependencies<'p>) {
        let deps = package.dependencies_to_install(install_suggestions);

        let res = Self {
            dependencies: deps
                .direct
                .iter()
                .map(|d| Cow::Owned(d.name().to_string()))
                .collect(),
            suggests: deps
                .suggests
                .iter()
                .map(|s| Cow::Owned(s.name().to_string()))
                .collect(),
            kind: PackageType::Source,
            force_source: true,
            install_suggests: install_suggestions,
            installation_status: cache.get_git_installation_status(repo_url, &sha),
            path: None,
            found_in_lockfile: false,
            name: Cow::Owned(package.name.clone()),
            version: Cow::Owned(package.version.original.clone()),
            source: Source::Git {
                git: repo_url.to_string(),
                commit: Some(sha),
                tag: None,
                branch: None,
                directory: None,
            },
        };

        (res, deps)
    }
}

impl<'a> fmt::Display for ResolvedDependency<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}={} ({}, type={}, from_lockfile={}, path='{}')",
            self.name,
            self.version,
            self.source,
            self.kind,
            self.found_in_lockfile,
            self.path.as_deref().unwrap_or("")
        )
    }
}

#[derive(Debug, PartialEq, Clone)]
enum UnresolvedDependencyKind<'d> {
    /// The user provided a dependency that doesn't exist
    Direct,
    /// A package has a dependency not found. It could be nested several times,
    /// we only show the immediate parent which could be an indirect dep as well.
    Indirect(Cow<'d, str>),
}

#[derive(Debug, PartialEq, Clone)]
pub struct UnresolvedDependency<'d> {
    name: Cow<'d, str>,
    // source: Option<&'d Source>,
    // error: Option<String>,
    version_requirement: Option<VersionRequirement>,
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
            if let Some(l) = &self.version_requirement {
                format!(" {l} ")
            } else {
                String::new()
            },
            format!("from: {}", origins.join(", "))
        )
    }
}

fn read_local_description_file(
    folder: impl AsRef<Path>,
) -> Result<Package, Box<dyn std::error::Error>> {
    let folder = folder.as_ref();
    let description_path = folder.join(DESCRIPTION_FILENAME);

    match fs::read_to_string(&description_path) {
        Ok(content) => {
            if let Some(package) = parse_description_file(&content) {
                Ok(package)
            } else {
                Err(format!("Invalid DESCRIPTION file at {}", description_path.display()).into())
            }
        }
        Err(e) => Err(format!(
            "Could not read destination file at {} {e}",
            description_path.display()
        )
        .into()),
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

    /// Tries to find all dependencies from the repos, as well as their install status
    pub fn resolve(
        &self,
        dependencies: &'d [DependencyKind],
        cache: &'d impl Cache,
        git_ops: &'d impl GitOperations,
    ) -> (Vec<ResolvedDependency<'d>>, Vec<UnresolvedDependency<'d>>) {
        let mut resolved = Vec::new();
        // We might have the same unresolved dep multiple times.
        let mut unresolved = HashMap::<_, UnresolvedDependency>::new();
        let mut found = HashSet::with_capacity(dependencies.len() * 10);

        let mut queue: VecDeque<_> = dependencies
            .iter()
            .map(|d| {
                (
                    Cow::Borrowed(d.name()),
                    d.as_lockfile_source(),
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
            pkg_source,
            version_requirement,
            install_suggestions,
            pkg_force_source,
            parent,
        )) = queue.pop_front()
        {
            // If we have already found that dependency, skip it
            // TODO: maybe different version req? we can cross that bridge later
            if found.contains(name.as_ref()) {
                continue;
            }

            // Look at lockfile before looking up any repositories
            if let Some(lockfile) = self.lockfile {
                // If we found the package in the lockfile, consider it found and do not look up
                // the repo at all
                if let Some(package) = lockfile.get_package(
                    name.as_ref(),
                    pkg_force_source,
                    install_suggestions,
                    pkg_source.as_ref(),
                ) {
                    found.insert(name.to_string());
                    resolved.push(ResolvedDependency::from_locked_package(package, cache));

                    for d in package.dependencies.iter().chain(&package.suggests) {
                        if !found.contains(d.as_str()) {
                            queue.push_back((
                                Cow::Borrowed(d.as_str()),
                                None,
                                None,
                                false,
                                false,
                                Some(name.clone()),
                            ));
                        }
                    }

                    continue;
                }
            }

            // For git and local sources, we could have errors (eg impossible to access repo, folder
            // not found) but we want to try to resolve everything rather than returning early
            let mut error = None;
            match pkg_source {
                None | Some(Source::Repository { .. }) => {
                    let repository = pkg_source.and_then(|c| c.r_repository());

                    for (repo, repo_source_only) in self.repositories {
                        if let Some(ref r) = repository {
                            if repo.name != *r {
                                continue;
                            }
                        }

                        if let Some((package, package_type)) = repo.find_package(
                            name.as_ref(),
                            version_requirement.as_ref(),
                            self.r_version,
                            pkg_force_source || *repo_source_only,
                        ) {
                            found.insert(name.to_string());
                            let (resolved_dep, deps) = ResolvedDependency::from_package_repository(
                                package,
                                &repo.url,
                                package_type,
                                install_suggestions,
                                pkg_force_source || *repo_source_only,
                                cache,
                            );

                            // deps.suggests will be empty if we don't have install_suggests=True
                            for d in deps.direct.into_iter().chain(deps.suggests) {
                                if !found.contains(d.name()) {
                                    queue.push_back((
                                        Cow::Borrowed(d.name()),
                                        None,
                                        d.version_requirement().map(|v| v.clone()),
                                        false,
                                        false,
                                        Some(name.clone()),
                                    ));
                                }
                            }
                            resolved.push(resolved_dep);
                            break;
                        }
                    }
                }
                // For local and git deps we assume they are source and will need to read the
                // DESCRIPTION file to get their dependencies
                Some(Source::Local { path }) => match read_local_description_file(path) {
                    Ok(package) => {
                        let deps = package.dependencies_to_install(install_suggestions);
                        found.insert(package.name);
                        todo!("handle local deps");
                    }
                    Err(e) => {
                        error = Some(format!("{e}"));
                    }
                },
                Some(Source::Git {
                    ref git,
                    ref tag,
                    ref branch,
                    ref commit,
                    directory,
                }) => {
                    let git_ref = if let Some(c) = commit {
                        GitReference::Commit(c)
                    } else if let Some(b) = branch {
                        GitReference::Branch(b)
                    } else if let Some(t) = tag {
                        GitReference::Tag(t)
                    } else {
                        unreachable!("Got an empty git reference")
                    };

                    let clone_path = cache.get_git_clone_path(git);
                    match git_ops.clone_and_checkout(git, git_ref.clone(), &clone_path) {
                        Ok(sha) => match read_local_description_file(clone_path) {
                            Ok(package) => {
                                let (resolved_dep, deps) = ResolvedDependency::from_git_package(
                                    &package,
                                    &git,
                                    sha,
                                    install_suggestions,
                                    cache,
                                );

                                // deps.suggests will be empty if we don't have install_suggests=True
                                for d in deps.direct.into_iter().chain(deps.suggests) {
                                    if !found.contains(d.name()) {
                                        queue.push_back((
                                            Cow::Owned(d.name().to_string()),
                                            None,
                                            d.version_requirement().map(|v| v.clone()),
                                            false,
                                            false,
                                            Some(name.clone()),
                                        ));
                                    }
                                }
                                resolved.push(resolved_dep);
                                continue;
                            }
                            Err(e) => {
                                error = Some(format!("{e}"));
                            }
                        },
                        Err(e) => {
                            error = Some(format!(
                                "Could not clone repository {git} (ref: {:?}) {e}",
                                git_ref,
                            ))
                        }
                    }
                }
            }

            // We don't look at repo for git and local packages
            if !found.contains(name.as_ref()) {
                let ud_kind = if let Some(p) = parent {
                    UnresolvedDependencyKind::Indirect(p)
                } else {
                    UnresolvedDependencyKind::Direct
                };
                if let Some(ud) = unresolved.get_mut(&name) {
                    ud.origins.push(ud_kind);
                } else {
                    unresolved.insert(
                        name.clone(),
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
            let all_deps = lockfile.get_package_tree(
                d.name(),
                d.force_source(),
                d.install_suggestions(),
                d.as_lockfile_source().as_ref(),
            );
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
    use crate::repository::RepositoryDatabase;
    use crate::CacheEntry;
    use git2::Error;
    use serde::Deserialize;
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

        fn get_git_installation_status(&self, _: &str, _: &str) -> InstallationStatus {
            InstallationStatus::Absent
        }

        fn get_git_clone_path(&self, _: &str) -> PathBuf {
            PathBuf::from("")
        }
    }

    struct FakeGit;

    impl GitOperations for FakeGit {
        fn clone_and_checkout(
            &self,
            url: &str,
            git_ref: GitReference<'_>,
            destination: impl AsRef<Path>,
        ) -> Result<String, Error> {
            Ok("abc".to_string())
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
        let r_version = config.r_version().clone();
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
            let (resolved, unresolved) =
                resolver.resolve(&config.dependencies(), &FakeCache {}, &FakeGit {});
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
