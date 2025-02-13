use crate::VersionRequirement;
use crate::{Cache, ConfigDependency, GitOperations, Lockfile, RepositoryDatabase, Version};

use std::borrow::Cow;
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

mod dependency;

use crate::cache::InstallationStatus;
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

impl<'d> Resolution<'d> {
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
    force_source: bool,
    parent: Option<Cow<'d, str>>,
    remote: Option<PackageRemote>,
    local_path: Option<PathBuf>,
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
    ($resolved:expr, $deps:expr) => {{
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

    fn local_lookup(
        &self,
        item: &QueueItem<'d>,
    ) -> Result<(ResolvedDependency<'d>, Vec<QueueItem<'d>>), Box<dyn std::error::Error>> {
        let local_path = item.local_path.as_ref().unwrap();
        if !local_path.exists() || !local_path.is_dir() {
            return Err(format!(
                "{} doesn't exist or is not a directory",
                local_path.display()
            )
            .into());
        }

        let package = parse_description_file_in_folder(local_path)?;
        let (resolved_dep, deps) = ResolvedDependency::from_local_package(
            &package,
            Source::Local {
                path: local_path.clone(),
            },
            item.install_suggestions,
        );
        Ok(prepare_deps!(resolved_dep, deps))
    }

    fn lockfile_lookup(
        &self,
        item: &QueueItem<'d>,
        cache: &'d impl Cache,
    ) -> Option<(ResolvedDependency<'d>, Vec<QueueItem<'d>>)> {
        if let Some(package) = self
            .lockfile
            .and_then(|l| l.get_package(&item.name, item.dep))
        {
            let installation_status = match &package.source {
                Source::Git { git, sha, .. } => {
                    cache.get_git_installation_status(&git, &sha, &package.name)
                }
                Source::Url { url, sha } => {
                    cache.get_url_installation_status(&url, &sha, &package.name)
                }
                Source::Repository { ref repository } => cache.get_package_installation_status(
                    repository.as_str(),
                    &package.name,
                    &package.version,
                ),
                // TODO: handle local
                Source::Local { .. } => InstallationStatus::Absent,
            };
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
        cache: &'d impl Cache,
    ) -> Option<(ResolvedDependency<'d>, Vec<QueueItem<'d>>)> {
        let repository = item.dep.as_ref().and_then(|c| c.r_repository());

        for (repo, repo_source_only) in self.repositories {
            if let Some(r) = repository {
                if repo.url != r {
                    continue;
                }
            }
            let force_source = item.force_source || *repo_source_only;

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
                    cache.get_package_installation_status(
                        &repo.url,
                        &package.name,
                        &package.version.original,
                    ),
                );
                return Some(prepare_deps!(resolved_dep, deps));
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
        cache: &'d impl Cache,
    ) -> Result<(ResolvedDependency<'d>, Vec<QueueItem<'d>>), Box<dyn std::error::Error>> {
        log::debug!("Cloning {repo_url} with ref {git_ref:?}");
        let clone_path = cache.get_git_clone_path(repo_url);

        match git_ops.clone_and_checkout(repo_url, git_ref.clone(), &clone_path) {
            Ok(sha) => {
                let package_path = if let Some(d) = directory {
                    clone_path.join(d)
                } else {
                    clone_path
                };
                let package = parse_description_file_in_folder(&package_path)?;
                println!("Repo {repo_url}:{sha}");
                let status = cache.get_git_installation_status(repo_url, &sha, &package.name);
                println!("Status for {} {:?}", package.name, status);

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
                let (resolved_dep, deps) = ResolvedDependency::from_git_package(
                    &package,
                    source,
                    item.install_suggestions,
                    status,
                );
                Ok(prepare_deps!(resolved_dep, deps))
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
        cache: &'d impl Cache,
        http_downloader: &'d impl HttpDownload,
    ) -> Result<(ResolvedDependency<'d>, Vec<QueueItem<'d>>), Box<dyn std::error::Error>> {
        let out_path = cache.get_url_download_path(url);
        let (dir, sha) = http_downloader.download_and_untar(url, &out_path, true)?;

        let install_path = dir.unwrap_or_else(|| out_path.clone());
        let package = parse_description_file_in_folder(&install_path)?;
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
        Ok(prepare_deps!(resolved_dep, deps))
    }

    /// Tries to find all dependencies from the repos, as well as their installation status
    pub fn resolve(
        &self,
        dependencies: &'d [ConfigDependency],
        prefer_repositories_for: &'d [String],
        cache: &'d impl Cache,
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
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::str::FromStr;

    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine;
    use git2::Error;
    use serde::Deserialize;
    use tempfile::TempDir;

    use crate::cache::InstallationStatus;
    use crate::config::Config;
    use crate::consts::DESCRIPTION_FILENAME;
    use crate::http::HttpError;
    use crate::repository::RepositoryDatabase;
    use crate::CacheEntry;

    struct FakeCache {
        cache_dir: TempDir,
    }

    impl FakeCache {
        pub fn new() -> Self {
            Self {
                cache_dir: tempfile::tempdir().unwrap(),
            }
        }
    }

    impl Cache for FakeCache {
        fn get_package_db_entry(&self, _: &str) -> CacheEntry {
            CacheEntry::NotFound(PathBuf::from_str("").unwrap())
        }

        fn get_package_installation_status(&self, _: &str, _: &str, _: &str) -> InstallationStatus {
            InstallationStatus::Absent
        }

        fn get_git_installation_status(&self, _: &str, _: &str, _: &str) -> InstallationStatus {
            InstallationStatus::Absent
        }

        fn get_url_installation_status(&self, _: &str, _: &str, _: &str) -> InstallationStatus {
            InstallationStatus::Absent
        }

        fn get_git_clone_path(&self, repo_url: &str) -> PathBuf {
            self.cache_dir
                .path()
                .join(STANDARD_NO_PAD.encode(repo_url.to_ascii_lowercase()))
        }

        fn get_url_download_path(&self, url: &str) -> PathBuf {
            self.cache_dir
                .path()
                .join(STANDARD_NO_PAD.encode(url.to_ascii_lowercase()))
        }
    }

    struct FakeGit;

    impl GitOperations for FakeGit {
        fn clone_and_checkout(
            &self,
            url: &str,
            git_ref: Option<GitReference<'_>>,
            destination: impl AsRef<Path>,
        ) -> Result<String, Error> {
            Ok("abc".to_string())
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

    #[test]
    fn resolving() {
        let paths = std::fs::read_dir("src/tests/resolution/").unwrap();
        let cache = FakeCache::new();
        // Add the DESCRIPTION file for git deps
        let remotes = vec![
            ("gsm", "https://github.com/Gilead-BioStats/gsm"),
            ("clindata", "https://github.com/Gilead-BioStats/clindata"),
            ("gsm.app", "https://github.com/Gilead-BioStats/gsm.app"),
        ];

        for (dep, url) in &remotes {
            let cache_path = cache.get_git_clone_path(url);
            std::fs::create_dir_all(&cache_path).unwrap();
            std::fs::copy(
                &format!("src/tests/descriptions/{dep}.DESCRIPTION"),
                cache_path.join(DESCRIPTION_FILENAME),
            )
            .unwrap();
        }

        // And a custom one for url deps
        let url = "https://cran.r-project.org/src/contrib/Archive/dplyr/dplyr_1.1.3.tar.gz";
        let url_path = cache.get_url_download_path(url);
        std::fs::create_dir_all(&url_path).unwrap();
        std::fs::copy(
            "src/tests/descriptions/dplyr.DESCRIPTION",
            url_path.join(DESCRIPTION_FILENAME),
        )
        .unwrap();

        for path in paths {
            let p = path.unwrap().path();
            let (config, r_version, repositories, lockfile) = extract_test_elements(&p);
            let resolver = Resolver::new(&repositories, &r_version, Some(&lockfile));
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
                out.push_str(&d.to_string());
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
