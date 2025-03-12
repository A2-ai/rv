use crate::VersionRequirement;
use crate::{ConfigDependency, DiskCache, GitOperations, Lockfile, RepositoryDatabase, Version};

use fs_err as fs;
use std::borrow::Cow;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

mod dependency;

use crate::fs::untar_archive;
use crate::git::GitReference;
use crate::http::HttpDownload;
use crate::lockfile::Source;
use crate::package::{
    is_binary_package, parse_description_file_in_folder, PackageRemote, PackageType,
};
pub use dependency::{ResolvedDependency, UnresolvedDependency};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resolution<'d> {
    pub found: Vec<ResolvedDependency<'d>>,
    pub failed: Vec<UnresolvedDependency<'d>>,
}

impl Resolution<'_> {
    pub fn is_success(&self) -> bool {
        self.failed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct QueueItem<'d> {
    name: Cow<'d, str>,
    dep: Option<&'d ConfigDependency>,
    pub(crate) version_requirement: Option<Cow<'d, VersionRequirement>>,
    install_suggestions: bool,
    force_source: Option<bool>,
    parent: Option<Cow<'d, str>>,
    remote: Option<PackageRemote>,
    local_path: Option<PathBuf>,
    // Only for top level dependencies. Checks whether the config dependency is matching
    // what we have in the lockfile, we have one.
    matching_in_lockfile: Option<bool>,
}

impl<'d> QueueItem<'d> {
    fn name_and_parent_only(name: Cow<'d, str>, parent: Cow<'d, str>) -> Self {
        Self {
            name,
            parent: Some(parent),
            ..Default::default()
        }
    }
}

// Macro to go around borrow errors we would get with a normal fn
macro_rules! prepare_deps {
    ($resolved:expr, $deps:expr, $matching_in_lockfile:expr) => {{
        let items = $deps
            .direct
            .into_iter()
            .chain($deps.suggests)
            .map(|p| {
                let mut i = QueueItem::name_and_parent_only(
                    Cow::Owned(p.name().to_string()),
                    $resolved.name.clone(),
                );

                i.version_requirement = p.version_requirement().map(|x| Cow::Owned(x.clone()));
                i.matching_in_lockfile = $matching_in_lockfile;

                for (pkg_name, remote) in $resolved.remotes.values() {
                    if let Some(n) = pkg_name {
                        if p.name() == n.as_str() {
                            i.remote = Some(remote.clone());
                        }
                    }
                }
                i
            })
            .collect();

        ($resolved, items)
    }};
}

#[derive(Debug, PartialEq)]
pub struct Resolver<'d> {
    /// We need that to resolve properly local deps relative to the project dir
    project_dir: PathBuf,
    /// The repositories are stored in the order defined in the config
    /// The last should get priority over previous repositories
    /// (db, force_source)
    repositories: &'d [(RepositoryDatabase, bool)],
    r_version: &'d Version,
    /// If we have a lockfile for the resolver, we will skip looking at the database for any package
    /// listed in it
    lockfile: Option<&'d Lockfile>,
}

impl<'d> Resolver<'d> {
    pub fn new(
        project_dir: impl AsRef<Path>,
        repositories: &'d [(RepositoryDatabase, bool)],
        r_version: &'d Version,
        lockfile: Option<&'d Lockfile>,
    ) -> Self {
        Self {
            project_dir: project_dir.as_ref().into(),
            repositories,
            r_version,
            lockfile,
        }
    }

    fn local_lookup(
        &self,
        item: &QueueItem<'d>,
    ) -> Result<(ResolvedDependency<'d>, Vec<QueueItem<'d>>), Box<dyn std::error::Error>> {
        let local_path = item.local_path.as_ref().unwrap();
        let canon_path = match fs::canonicalize(self.project_dir.join(local_path)) {
            Ok(canon_path) => canon_path,
            Err(_) => return Err(format!("{} doesn't exist.", local_path.display()).into()),
        };

        let (package, sha) = if canon_path.is_file() {
            // We have a file, it should be a tarball.
            // even though we might have to extract again in sync?
            let tempdir = tempfile::tempdir()?;
            let (path, hash) =
                untar_archive(fs::read(&canon_path)?.as_slice(), tempdir.path(), true)?;
            (
                parse_description_file_in_folder(path.unwrap_or_else(|| canon_path.clone()))?,
                hash,
            )
        } else if canon_path.is_dir() {
            // we have a folder
            (parse_description_file_in_folder(&canon_path)?, None)
        } else {
            unreachable!()
        };

        if item.name != package.name {
            return Err(format!(
                "Found package `{}` from {} but it is called `{}` in the rproject.toml",
                package.name,
                local_path.display(),
                item.name
            )
            .into());
        }

        let (resolved_dep, deps) = ResolvedDependency::from_local_package(
            &package,
            Source::Local {
                path: local_path.clone(),
                sha,
            },
            item.install_suggestions,
            canon_path,
        );
        Ok(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile))
    }

    fn lockfile_lookup(
        &self,
        item: &QueueItem<'d>,
        cache: &'d DiskCache,
    ) -> Option<(ResolvedDependency<'d>, Vec<QueueItem<'d>>)> {
        // If the dependency is not matching, do not even look at the lockfile
        if let Some(matching) = item.matching_in_lockfile {
            if !matching {
                return None;
            }
        }

        if let Some(package) = self
            .lockfile
            .and_then(|l| l.get_package(&item.name, item.dep))
        {
            let installation_status =
                cache.get_installation_status(&item.name, &package.version, &package.source);
            let resolved_dep =
                ResolvedDependency::from_locked_package(package, installation_status);
            let items = package
                .dependencies
                .iter()
                .chain(&package.suggests)
                .map(|p| QueueItem::name_and_parent_only(Cow::Borrowed(p), item.name.clone()))
                .collect();
            Some((resolved_dep, items))
        } else {
            None
        }
    }

    fn repositories_lookup(
        &self,
        item: &QueueItem<'d>,
        cache: &'d DiskCache,
    ) -> Option<(ResolvedDependency<'d>, Vec<QueueItem<'d>>)> {
        let repository = item.dep.as_ref().and_then(|c| c.r_repository());

        for (repo, repo_source_only) in self.repositories {
            if let Some(r) = repository {
                if repo.url != r {
                    continue;
                }
            }
            let force_source = if let Some(source) = item.force_source {
                source
            } else {
                *repo_source_only
            };

            if let Some((package, package_type)) = repo.find_package(
                item.name.as_ref(),
                item.version_requirement.as_deref(),
                self.r_version,
                force_source,
            ) {
                let (resolved_dep, deps) = ResolvedDependency::from_package_repository(
                    package,
                    &repo.url,
                    package_type,
                    item.install_suggestions,
                    force_source,
                    cache.get_installation_status(
                        &package.name,
                        &package.version.original,
                        &Source::Repository {
                            repository: repo.url.clone(),
                        },
                    ),
                );
                return Some(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile));
            }
        }

        None
    }

    fn git_lookup(
        &self,
        item: &QueueItem<'d>,
        repo_url: &str,
        directory: Option<&str>,
        git_ref: Option<GitReference>,
        git_ops: &'d impl GitOperations,
        cache: &'d DiskCache,
    ) -> Result<(ResolvedDependency<'d>, Vec<QueueItem<'d>>), Box<dyn std::error::Error>> {
        let clone_path = cache.get_git_clone_path(repo_url);

        match git_ops.clone_and_checkout(repo_url, git_ref.clone(), &clone_path) {
            Ok(sha) => {
                let package_path = if let Some(d) = directory {
                    clone_path.join(d)
                } else {
                    clone_path
                };
                let package = parse_description_file_in_folder(&package_path)?;

                if item.name != package.name {
                    return Err(format!(
                        "Found package `{}` from {repo_url} but it is called `{}` in the rproject.toml",
                        package.name, item.name
                    )
                    .into());
                }

                let source = if let Some(dep) = item.dep {
                    dep.as_git_source_with_sha(sha)
                } else {
                    // If it's coming from a remote, only store the sha
                    Source::Git {
                        git: repo_url.to_string(),
                        sha,
                        directory: None,
                        tag: None,
                        branch: None,
                    }
                };
                let status =
                    cache.get_installation_status(repo_url, &package.version.original, &source);
                let (resolved_dep, deps) = ResolvedDependency::from_git_package(
                    &package,
                    source,
                    item.install_suggestions,
                    status,
                );
                Ok(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile))
            }
            Err(e) => {
                Err(format!("Could not clone repository {repo_url} (ref: {git_ref:?}) {e}").into())
            }
        }
    }

    fn url_lookup(
        &self,
        item: &QueueItem<'d>,
        url: &str,
        cache: &'d DiskCache,
        http_downloader: &'d impl HttpDownload,
    ) -> Result<(ResolvedDependency<'d>, Vec<QueueItem<'d>>), Box<dyn std::error::Error>> {
        let out_path = cache.get_url_download_path(url);
        let (dir, sha) = http_downloader.download_and_untar(url, &out_path, true)?;

        let install_path = dir.unwrap_or_else(|| out_path.clone());
        let package = parse_description_file_in_folder(&install_path)?;
        if item.name != package.name {
            return Err(format!(
                "Found package `{}` from {url} but it is called `{}` in the rproject.toml",
                package.name, item.name
            )
            .into());
        }
        let is_binary = is_binary_package(&install_path, &package.name);
        let (resolved_dep, deps) = ResolvedDependency::from_url_package(
            &package,
            if is_binary {
                PackageType::Binary
            } else {
                PackageType::Source
            },
            Source::Url {
                url: url.to_string(),
                sha,
            },
            item.install_suggestions,
        );
        Ok(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile))
    }

    /// Tries to find all dependencies from the repos, as well as their installation status
    pub fn resolve(
        &self,
        dependencies: &'d [ConfigDependency],
        prefer_repositories_for: &'d [String],
        cache: &'d DiskCache,
        git_ops: &'d impl GitOperations,
        http_download: &'d impl HttpDownload,
    ) -> Resolution<'d> {
        let mut result = Resolution::default();
        let mut processed = HashSet::with_capacity(dependencies.len() * 10);
        let mut queue: VecDeque<_> = dependencies
            .iter()
            .map(|d| QueueItem {
                name: Cow::Borrowed(d.name()),
                dep: Some(d),
                version_requirement: None,
                install_suggestions: d.install_suggestions(),
                force_source: d.force_source(),
                parent: None,
                remote: None,
                local_path: d.local_path(),
                matching_in_lockfile: self
                    .lockfile
                    .map(|l| l.get_package(d.name(), Some(d)).is_some()),
            })
            .collect();

        while let Some(item) = queue.pop_front() {
            // If we have already found that dependency, skip it
            // TODO: maybe different version req? we can cross that bridge later
            if processed.contains(item.name.as_ref()) {
                continue;
            }

            // If we have a local path, we don't need to check anything at all, just the actual path
            if item.local_path.is_some() {
                match self.local_lookup(&item) {
                    Ok((resolved_dep, items)) => {
                        processed.insert(resolved_dep.name.to_string());
                        result.found.push(resolved_dep);
                        queue.extend(items);
                        continue;
                    }
                    Err(e) => result
                        .failed
                        .push(UnresolvedDependency::from_item(&item).with_error(format!("{e:?}"))),
                }
                continue;
            }

            // Look at lockfile before doing anything else
            if let Some((resolved_dep, items)) = self.lockfile_lookup(&item, cache) {
                processed.insert(resolved_dep.name.to_string());
                result.found.push(resolved_dep);
                queue.extend(items);
                continue;
            }

            // Then we handle it differently depending on the source but even if we fail to find
            // something, we will consider it processed
            processed.insert(item.name.to_string());

            // But first, we check if the item has a remote and use that instead
            // We will the remote result around _if_ the item has a version requirement and is in
            // override list so we can check in the repo before pushing the remote version
            let mut remote_result = None;
            // .contains would need to allocate, so using iter().any() instead
            let can_be_overridden = item.version_requirement.is_some()
                && prefer_repositories_for
                    .iter()
                    .any(|s| s == item.name.as_ref());

            if let Some(ref remote) = item.remote {
                match remote {
                    PackageRemote::Git {
                        url,
                        reference,
                        // TODO: support PR somehow
                        // pull_request,
                        directory,
                        ..
                    } => {
                        match self.git_lookup(
                            &item,
                            url,
                            directory.as_deref(),
                            reference.clone().as_deref().map(GitReference::Unknown),
                            git_ops,
                            cache,
                        ) {
                            Ok((mut resolved_dep, items)) => {
                                // TODO: do we want to keep track of the remote string?
                                resolved_dep.from_remote = true;
                                if can_be_overridden {
                                    remote_result = Some((resolved_dep, items));
                                } else {
                                    result.found.push(resolved_dep);
                                    queue.extend(items);
                                }
                            }
                            Err(e) => {
                                result.failed.push(
                                    UnresolvedDependency::from_item(&item)
                                        .with_error(format!("{e:?}"))
                                        .with_remote(remote.clone()),
                                );
                            }
                        }
                    }
                    _ => {
                        result.failed.push(
                            UnresolvedDependency::from_item(&item)
                                .with_error("Remote not supported".to_string())
                                .with_remote(remote.clone()),
                        );
                    }
                }
                if remote_result.is_none() {
                    continue;
                }
            }

            match item.dep {
                None
                | Some(ConfigDependency::Detailed { .. })
                | Some(ConfigDependency::Simple(_)) => {
                    if let Some((resolved_dep, items)) = self.repositories_lookup(&item, cache) {
                        result.found.push(resolved_dep);
                        queue.extend(items);
                    } else {
                        // Fallback to the remote result otherwise
                        if let Some((resolved_dep, items)) = remote_result {
                            result.found.push(resolved_dep);
                            queue.extend(items);
                        } else {
                            log::debug!("Didn't find {}", item.name);
                            result.failed.push(UnresolvedDependency::from_item(&item));
                        }
                    }
                }
                Some(ConfigDependency::Url { url, .. }) => {
                    match self.url_lookup(&item, url.as_ref(), cache, http_download) {
                        Ok((resolved_dep, items)) => {
                            result.found.push(resolved_dep);
                            queue.extend(items);
                        }
                        Err(e) => {
                            result.failed.push(
                                UnresolvedDependency::from_item(&item)
                                    .with_error(format!("{e:?}"))
                                    .with_url(url.as_str()),
                            );
                        }
                    }
                }
                Some(ConfigDependency::Local { .. }) => unreachable!("handled beforehand"),
                Some(ConfigDependency::Git {
                    git,
                    tag,
                    commit,
                    branch,
                    directory,
                    ..
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

                    match self.git_lookup(
                        &item,
                        git,
                        directory.as_deref(),
                        Some(git_ref),
                        git_ops,
                        cache,
                    ) {
                        Ok((resolved_dep, items)) => {
                            result.found.push(resolved_dep);
                            queue.extend(items);
                        }
                        Err(e) => {
                            result.failed.push(
                                UnresolvedDependency::from_item(&item).with_error(format!("{e:?}")),
                            );
                        }
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::str::FromStr;

    use git2::Error;
    use serde::Deserialize;
    use tempfile::TempDir;

    use crate::config::Config;
    use crate::consts::DESCRIPTION_FILENAME;
    use crate::http::HttpError;
    use crate::package::{parse_package_file, Package};
    use crate::repository::RepositoryDatabase;
    use crate::{DiskCache, SystemInfo};

    struct FakeGit;

    impl GitOperations for FakeGit {
        fn clone_and_checkout(
            &self,
            _: &str,
            _: Option<GitReference<'_>>,
            _: impl AsRef<Path>,
        ) -> Result<String, Error> {
            Ok("somethinglikeasha".to_string())
        }
    }

    struct FakeHttp;

    impl HttpDownload for FakeHttp {
        fn download<W: Write>(
            &self,
            _: &str,
            _: &mut W,
            _: Vec<(&str, String)>,
        ) -> Result<u64, HttpError> {
            Ok(0)
        }

        fn download_and_untar(
            &self,
            _: &str,
            _: impl AsRef<Path>,
            _: bool,
        ) -> Result<(Option<PathBuf>, String), HttpError> {
            Ok((None, "SOME_SHA".to_string()))
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
        dbs: &HashMap<String, HashMap<String, Vec<Package>>>,
    ) -> (Config, Version, Vec<(RepositoryDatabase, bool)>, Lockfile) {
        let content = std::fs::read_to_string(path).unwrap();
        let parts: Vec<_> = content.splitn(3, "---").collect();
        let config = Config::from_str(parts[0]).expect("valid config");
        let r_version = config.r_version().clone();
        let repositories = if let Ok(data) = toml::from_str::<TestRepositories>(parts[1]) {
            let mut res = Vec::new();
            for r in data.repos {
                let mut repo = RepositoryDatabase::new(&format!("http://{}", r.name));
                if let Some(p) = r.source {
                    repo.source_packages = dbs[&p].clone();
                }

                if let Some(p) = r.binary {
                    repo.binary_packages
                        .insert(r_version.major_minor(), dbs[&p].clone());
                }
                res.push((repo, r.force_source));
            }
            res
        } else {
            let mut repo = RepositoryDatabase::new("");
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

    fn setup_cache(r_version: &Version) -> (TempDir, DiskCache) {
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = DiskCache::new_in_dir(
            r_version,
            SystemInfo::from_os_info(),
            cache_dir.path().to_path_buf(),
        )
        .unwrap();

        // Add the DESCRIPTION file for git deps
        let remotes = vec![
            ("gsm", "https://github.com/Gilead-BioStats/gsm"),
            ("clindata", "https://github.com/Gilead-BioStats/clindata"),
            ("gsm.app", "https://github.com/Gilead-BioStats/gsm.app"),
        ];

        for (dep, url) in &remotes {
            let cache_path = cache.get_git_clone_path(url);
            fs::create_dir_all(&cache_path).unwrap();
            fs::copy(
                &format!("src/tests/descriptions/{dep}.DESCRIPTION"),
                cache_path.join(DESCRIPTION_FILENAME),
            )
            .unwrap();
        }

        // And a custom one for url deps
        let url = "https://cran.r-project.org/src/contrib/Archive/dplyr/dplyr_1.1.3.tar.gz";
        let url_path = cache.get_url_download_path(url);
        fs::create_dir_all(&url_path).unwrap();
        fs::copy(
            "src/tests/descriptions/dplyr.DESCRIPTION",
            url_path.join(DESCRIPTION_FILENAME),
        )
        .unwrap();

        (cache_dir, cache)
    }

    #[test]
    fn resolving() {
        let paths = std::fs::read_dir("src/tests/resolution/").unwrap();
        let dbs: HashMap<_, _> = std::fs::read_dir("src/tests/package_files/")
            .unwrap()
            .into_iter()
            .map(|x| {
                let x = x.unwrap();
                let content = std::fs::read_to_string(x.path()).unwrap();
                (
                    x.file_name()
                        .to_string_lossy()
                        .trim_end_matches(".PACKAGE")
                        .to_string(),
                    parse_package_file(content.as_str()),
                )
            })
            .collect();

        for path in paths {
            let p = path.unwrap().path();
            let (config, r_version, repositories, lockfile) = extract_test_elements(&p, &dbs);
            let (_cache_dir, cache) = setup_cache(&r_version);
            let resolver =
                Resolver::new(Path::new("."), &repositories, &r_version, Some(&lockfile));
            let resolution = resolver.resolve(
                &config.dependencies(),
                config.prefer_repositories_for(),
                &cache,
                &FakeGit {},
                &FakeHttp {},
            );
            // let new_lockfile = Lockfile::from_resolved(&r_version.major_minor(), &resolved);
            // println!("{}", new_lockfile.as_toml_string());
            let mut out = String::new();
            for d in resolution.found {
                out.push_str(&format!("{d:?}"));
                out.push_str("\n");
            }

            if !resolution.failed.is_empty() {
                out.push_str("--- unresolved --- \n");
                for d in resolution.failed {
                    out.push_str(&d.to_string());
                    out.push_str("\n");
                }
            }
            // Output has been compared with pkgr for the same PACKAGE file
            insta::assert_snapshot!(p.file_name().unwrap().to_string_lossy().to_string(), out);
        }
    }
}
