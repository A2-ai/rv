use crate::VersionRequirement;
use crate::{CommandExecutor, ConfigDependency, DiskCache, Lockfile, RepositoryDatabase, Version};

use fs_err as fs;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;

mod dependency;
mod result;
mod sat;

use crate::fs::untar_archive;
use crate::git::url::GitUrl;
use crate::git::{GitReference, GitRemote};
use crate::http::HttpDownload;
use crate::lockfile::Source;
use crate::package::{
    Package, PackageRemote, PackageType, is_binary_package, parse_description_file,
    parse_description_file_in_folder,
};
use crate::utils::create_spinner;
pub use dependency::{ResolvedDependency, UnresolvedDependency};
pub use result::Resolution;

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
    fn has_required_repo(&self) -> bool {
        self.dep.is_some_and(|d| match d {
            ConfigDependency::Detailed { repository, .. } => repository.is_some(),
            _ => false,
        })
    }

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
    /// We might not have loaded the databases but we still want their urls
    repo_urls: HashSet<&'d str>,
    r_version: &'d Version,
    /// The base + recommended package versions for the R version we are using
    builtin_packages: &'d HashMap<String, Package>,
    /// Env vars from the config
    packages_env_vars: &'d HashMap<String, HashMap<String, String>>,
    /// If we have a lockfile for the resolver, we will skip looking at the database for any package
    /// listed in it
    lockfile: Option<&'d Lockfile>,
    /// Progress bar is only shown for git dependencies
    show_progress_bar: bool,
}

impl<'d> Resolver<'d> {
    pub fn new(
        project_dir: impl AsRef<Path>,
        repositories: &'d [(RepositoryDatabase, bool)],
        repo_urls: HashSet<&'d str>,
        r_version: &'d Version,
        builtin_packages: &'d HashMap<String, Package>,
        lockfile: Option<&'d Lockfile>,
        packages_env_vars: &'d HashMap<String, HashMap<String, String>>,
    ) -> Self {
        Self {
            project_dir: project_dir.as_ref().into(),
            repositories,
            repo_urls,
            r_version,
            lockfile,
            builtin_packages,
            packages_env_vars,
            show_progress_bar: false,
        }
    }

    pub fn show_progress_bar(&mut self) {
        self.show_progress_bar = true;
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
            // For some type of packages we will always refresh directly from the source
            // eg a branch might have added commits
            if package.source.could_have_changed() {
                return None;
            }

            if let Some(req) = &item.version_requirement {
                if !req.is_satisfied(&Version::from_str(&package.version).unwrap()) {
                    return None;
                }
            }

            let installation_status =
                cache.get_installation_status(&item.name, &package.version, &package.source);
            let resolved_dep =
                ResolvedDependency::from_locked_package(package, installation_status);

            let items = package
                .dependencies
                .iter()
                .chain(&package.suggests)
                .map(|p| {
                    QueueItem::name_and_parent_only(Cow::Borrowed(p.name()), item.name.clone())
                })
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
        http_download: &impl HttpDownload,
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
                    &Url::parse(&repo.url).unwrap(),
                    package_type,
                    item.install_suggestions,
                    force_source,
                    cache.get_installation_status(
                        &package.name,
                        &package.version.original,
                        &Source::Repository {
                            repository: Url::parse(&repo.url).unwrap(),
                        },
                    ),
                    http_download,
                );
                return Some(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile));
            }
        }

        None
    }

    fn git_lookup(
        &self,
        item: &QueueItem<'d>,
        repo_url: &GitUrl,
        directory: Option<&str>,
        git_ref: GitReference,
        git_executor: &'d (impl CommandExecutor + Clone + 'static),
        cache: &'d DiskCache,
    ) -> Result<(ResolvedDependency<'d>, Vec<QueueItem<'d>>), Box<dyn std::error::Error>> {
        let clone_path = cache.get_git_clone_path(repo_url.url());

        let mut remote = GitRemote::new(repo_url.url());
        if let Some(d) = directory {
            remote.set_directory(d);
        }

        let spinner = create_spinner(
            self.show_progress_bar,
            format!("Fetching DESCRIPTION file from {repo_url}#{git_ref}"),
        );

        match remote.sparse_checkout_for_description(clone_path, &git_ref, git_executor.clone()) {
            Ok((sha, description_content)) => {
                spinner.finish_and_clear();
                let package = match parse_description_file(&description_content) {
                    Some(p) => p,
                    None => {
                        return Err(format!(
                            "DESCRIPTION file from {repo_url} was found but is not valid",
                        )
                        .into());
                    }
                };

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
                        git: repo_url.clone(),
                        sha,
                        directory: None,
                        tag: None,
                        branch: None,
                    }
                };
                let status = cache.get_installation_status(
                    &package.name,
                    &package.version.original,
                    &source,
                );
                let (resolved_dep, deps) = ResolvedDependency::from_git_package(
                    &package,
                    source,
                    item.install_suggestions,
                    status,
                );
                Ok(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile))
            }
            Err(e) => {
                spinner.finish_and_clear();
                Err(format!("Could not fetch repository {repo_url} (ref: {git_ref:?}) {e}").into())
            }
        }
    }

    fn url_lookup(
        &self,
        item: &QueueItem<'d>,
        url: &Url,
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
                url: url.clone(),
                sha,
            },
            item.install_suggestions,
        );
        Ok(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile))
    }

    fn builtin_lookup(
        &self,
        item: &QueueItem<'d>,
    ) -> Option<(ResolvedDependency<'d>, Vec<QueueItem<'d>>)> {
        if let Some(package) = self.builtin_packages.get(item.name.as_ref()) {
            if let Some(ref req) = item.version_requirement {
                if req.is_satisfied(&package.version) {
                    let (resolved_dep, deps) =
                        ResolvedDependency::from_builtin_package(package, item.install_suggestions);
                    Some(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile))
                } else {
                    None
                }
            } else {
                // if there's no version requirement, we are fine with what's builtin
                let (resolved_dep, deps) =
                    ResolvedDependency::from_builtin_package(package, item.install_suggestions);
                Some(prepare_deps!(resolved_dep, deps, item.matching_in_lockfile))
            }
        } else {
            None
        }
    }

    /// Tries to find all dependencies from the repos, as well as their installation status
    pub fn resolve(
        &self,
        dependencies: &'d [ConfigDependency],
        prefer_repositories_for: &'d [String],
        cache: &'d DiskCache,
        git_exec: &'d (impl CommandExecutor + Clone + 'static),
        http_download: &'d impl HttpDownload,
    ) -> Resolution<'d> {
        let mut result = Resolution::default();
        let mut processed = HashSet::with_capacity(dependencies.len() * 10);
        // Top level dependencies can require specific repos.
        // We should not try to resolve those from anywhere else even if they dependencies of other
        // packages
        let repo_required: HashSet<_> = dependencies
            .iter()
            .filter(|d| d.r_repository().is_some())
            .map(|d| d.name())
            .collect();
        let dependencies_only: HashSet<_> = dependencies
            .iter()
            .filter(|d| d.dependencies_only())
            .map(|d| d.name())
            .collect();

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
                matching_in_lockfile: self.lockfile.and_then(|l| {
                    l.get_package(d.name(), Some(d))
                        .map(|p| p.is_matching(d, &self.repo_urls))
                }),
            })
            .collect();

        while let Some(item) = queue.pop_front() {
            // If we have already found that dependency and it has a forced repo, skip it
            if processed.contains(item.name.as_ref()) && repo_required.contains(item.name.as_ref())
            {
                continue;
            }

            // If we have a local path, we don't need to check anything at all, just the actual path
            if item.local_path.is_some() {
                match self.local_lookup(&item) {
                    Ok((resolved_dep, items)) => {
                        processed.insert(resolved_dep.name.to_string());
                        result.add_found(resolved_dep);
                        queue.extend(items);
                        continue;
                    }
                    Err(e) => result
                        .failed
                        .push(UnresolvedDependency::from_item(&item).with_error(format!("{e:?}"))),
                }
                continue;
            }

            // First let's check if it's a builtin package if the R version is matching if the package
            // is not listed from a specific repo
            if !item.has_required_repo() {
                if let Some((resolved_dep, items)) = self.builtin_lookup(&item) {
                    processed.insert(resolved_dep.name.to_string());
                    result.add_found(resolved_dep);
                    queue.extend(items);
                    continue;
                }
            }

            // Look at lockfile
            if let Some((resolved_dep, items)) = self.lockfile_lookup(&item, cache) {
                processed.insert(resolved_dep.name.to_string());
                result.add_found(resolved_dep);
                queue.extend(items);
                continue;
            }

            // Then we handle it differently depending on the source but even if we fail to find
            // something, we will consider it processed
            processed.insert(item.name.to_string());

            // But first, we check if the item has a remote and use that instead
            // We will keep the remote result around _if_ the item has a version requirement and is in
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
                            reference
                                .clone()
                                .as_deref()
                                .map(GitReference::Unknown)
                                .unwrap_or(GitReference::Unknown("HEAD")),
                            git_exec,
                            cache,
                        ) {
                            Ok((mut resolved_dep, items)) => {
                                // TODO: do we want to keep track of the remote string?
                                resolved_dep.from_remote = true;
                                if can_be_overridden {
                                    remote_result = Some((resolved_dep, items));
                                } else {
                                    result.add_found(resolved_dep);
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
                    // If we already have something that will satisfy the dependency, no need
                    // to look it up again
                    if item.version_requirement.is_none() && result.found_in_repo(&item.name) {
                        continue;
                    }
                    if let Some((resolved_dep, items)) =
                        self.repositories_lookup(&item, cache, http_download)
                    {
                        result.add_found(resolved_dep);
                        queue.extend(items);
                    } else {
                        // Fallback to the remote result otherwise
                        if let Some((resolved_dep, items)) = remote_result {
                            result.add_found(resolved_dep);
                            queue.extend(items);
                        } else {
                            log::debug!("Didn't find {}", item.name);
                            result.failed.push(UnresolvedDependency::from_item(&item));
                        }
                    }
                }
                Some(ConfigDependency::Url { url, .. }) => {
                    match self.url_lookup(&item, url, cache, http_download) {
                        Ok((resolved_dep, items)) => {
                            result.add_found(resolved_dep);
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
                        git_ref,
                        git_exec,
                        cache,
                    ) {
                        Ok((resolved_dep, items)) => {
                            result.add_found(resolved_dep);
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

        for name in dependencies_only {
            result.ignore(name);
        }

        for dep in result.found.iter_mut() {
            if let Some(args) = self.packages_env_vars.get(dep.name.as_ref()) {
                dep.env_vars = args.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            }
        }
        result.finalize();

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::str::FromStr;

    use serde::Deserialize;
    use tempfile::TempDir;

    use crate::config::Config;
    use crate::consts::{BASE_PACKAGES, DESCRIPTION_FILENAME};
    use crate::http::HttpError;
    use crate::package::{Package, parse_package_file};
    use crate::repository::RepositoryDatabase;
    use crate::{DiskCache, SystemInfo};

    #[derive(Clone)]
    struct FakeGit;

    impl CommandExecutor for FakeGit {
        fn execute(&self, _: &mut Command) -> Result<String, std::io::Error> {
            Ok("somethinglikeasha".to_string())
        }
    }

    struct FakeHttp;

    impl HttpDownload for FakeHttp {
        fn download<W: Write>(
            &self,
            url: &Url,
            w: &mut W,
            _: Vec<(&str, String)>,
        ) -> Result<u64, HttpError> {
            // if its an api query, we return the api string
            if url.as_str().contains("r-universe.dev/api") {
                let path = format!(
                    "src/tests/r_universe/{}.api",
                    url.as_str().split('/').last().unwrap_or("")
                );
                let content = fs::read_to_string(path).unwrap();

                w.write_all(content.as_bytes())
                    .map_err(|e| HttpError::from_io(url.as_str(), e))?;
            }
            Ok(0)
        }

        fn download_and_untar(
            &self,
            _: &Url,
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
                let mut repo = RepositoryDatabase::new(&format!("http://{}/", r.name));
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
            let mut repo = RepositoryDatabase::new("http://cran/");
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
            ("missing.remote", "https://github.com/dummy/missing.remote"),
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
        let url_path = cache.get_url_download_path(&Url::parse(&url).unwrap());
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
            // let r_cmd = RCommandLine { r: None };
            // let builtin_packages = cache.get_builtin_packages_versions(r_cmd.clone()).unwrap();
            let mut builtin_packages = HashMap::new();
            let mut survival = Package::default();
            survival.name = "survival".to_string();
            survival.version = Version::from_str("2.1.1").unwrap();
            builtin_packages.insert("survival".to_string(), survival);
            let mut mass = Package::default();
            mass.name = "MASS".to_string();
            mass.version = Version::from_str("7.3-60").unwrap();
            builtin_packages.insert("MASS".to_string(), mass);

            let resolver = Resolver::new(
                Path::new("."),
                &repositories,
                repositories.iter().map(|(x, _)| x.url.as_str()).collect(),
                &r_version,
                &builtin_packages,
                Some(&lockfile),
                config.packages_env_vars(),
            );

            let resolution = resolver.resolve(
                &config.dependencies(),
                config.prefer_repositories_for(),
                &cache,
                &FakeGit {},
                &FakeHttp {},
            );
            // let new_lockfile = Lockfile::from_resolved(&r_version.major_minor(), resolution.found.clone());
            // println!("{}", new_lockfile.as_toml_string());
            let mut out = String::new();
            // Base packages would be noise for the resolution
            for d in resolution
                .found
                .iter()
                .filter(|x| !BASE_PACKAGES.contains(&x.name.as_ref()))
            {
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

            if !resolution.req_failures.is_empty() {
                out.push_str("--- requirement failures --- \n");
                for (pkg_name, requirements) in resolution.req_failures {
                    out.push_str(&pkg_name);
                    out.push_str(" : ");
                    out.push_str(
                        &requirements
                            .iter()
                            .map(|x| x.to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    out.push_str("\n");
                }
            }
            // Output has been compared with pkgr for the same PACKAGE file
            insta::assert_snapshot!(p.file_name().unwrap().to_string_lossy().to_string(), out);
        }
    }
}
