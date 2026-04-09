use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::consts::{BASE_PACKAGES, RECOMMENDED_PACKAGES};
use crate::git::url::GitUrl;
use crate::lockfile::Source as LockSource;
use crate::{
    Config, Lockfile, Repository, RepositoryDatabase,
    package::{Operator, Version, VersionRequirement, deserialize_version, serialize_version},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use url::Url;

#[derive(Debug, PartialEq, Clone)]
// as enum since logic to resolve depends on this
enum RenvSource {
    Repository,
    Git,
    GitHub,
    Local,
    Other(String),
}

impl Serialize for RenvSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            RenvSource::Repository => serializer.serialize_str("Repository"),
            RenvSource::Git => serializer.serialize_str("Git"),
            RenvSource::GitHub => serializer.serialize_str("GitHub"),
            RenvSource::Local => serializer.serialize_str("Local"),
            RenvSource::Other(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for RenvSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let source_enum = match s.as_str() {
            "Repository" => RenvSource::Repository,
            "Git" => RenvSource::Git,
            "GitHub" => RenvSource::GitHub,
            "Local" => RenvSource::Local,
            other => RenvSource::Other(other.to_string()),
        };
        Ok(source_enum)
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct PackageInfo {
    package: String,
    #[serde(
        deserialize_with = "deserialize_version",
        serialize_with = "serialize_version"
    )]
    version: Version,
    source: RenvSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_subdir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    requirements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct RenvRepository {
    name: String,
    #[serde(rename = "URL")]
    url: String,
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct RInfo {
    #[serde(
        deserialize_with = "deserialize_version",
        serialize_with = "serialize_version"
    )]
    version: Version,
    repositories: Vec<RenvRepository>,
}

#[derive(Debug, PartialEq, Clone, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct RenvLock {
    r: RInfo,
    packages: BTreeMap<String, PackageInfo>,
}

impl RenvLock {
    pub fn parse_renv_lock<P: AsRef<Path>>(path: P) -> Result<Self, FromJsonFileError> {
        let path = path.as_ref();
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                return Err(FromJsonFileError {
                    path: path.into(),
                    source: FromJsonFileErrorKind::Io(e),
                });
            }
        };

        serde_json::from_str(content.as_str()).map_err(|e| FromJsonFileError {
            path: path.into(),
            source: FromJsonFileErrorKind::Parse(e),
        })
    }

    pub fn resolve(
        &self,
        repository_database: &[(RepositoryDatabase, bool)],
    ) -> (Vec<ResolvedRenv<'_>>, Vec<UnresolvedRenv>) {
        let mut resolved = Vec::new();
        let mut unresolved = Vec::new();
        for package_info in self.packages.values() {
            // if package is sourced from a repository and is a recommended package, do not attempt to resolve
            // TODO: add flag to resolve recommended packages
            if package_info.source == RenvSource::Repository
                && RECOMMENDED_PACKAGES.contains(&package_info.package.as_str())
            {
                continue;
            }

            let res = match &package_info.source {
                RenvSource::Repository => resolve_repository(
                    package_info,
                    &self.r.repositories,
                    repository_database,
                    &self.r.version,
                ),
                RenvSource::Git => resolve_git(package_info),
                RenvSource::GitHub => resolve_github(package_info),
                RenvSource::Local => resolve_local(package_info),
                RenvSource::Other(source) => {
                    Err(format!("Source ({source}) is not supported").into())
                }
            };
            match res {
                Ok(source) => resolved.push(ResolvedRenv {
                    package_info,
                    source,
                }),
                Err(error) => unresolved.push(UnresolvedRenv {
                    package_info: package_info.clone(),
                    error,
                }),
            }
        }

        // alphabetize to match with plan/sync
        resolved.sort_by_key(|a| &a.package_info.package);
        unresolved.sort_by_key(|a| a.package_info.package.clone());
        (resolved, unresolved)
    }

    pub fn r_version(&self) -> &Version {
        &self.r.version
    }

    pub fn config_repositories(&self) -> Vec<Repository> {
        self.r
            .repositories
            .iter()
            .map(|r| {
                Repository::new(
                    r.name.to_string(),
                    Url::parse(&r.url).expect("valid URL"),
                    false,
                )
            })
            .collect::<Vec<_>>()
    }
}

// Expected Repository sourced package format from renv.lock
// "R6": {
//     "Package": "R6",
//     "Version": "2.5.1",
//     "Source": "Repository",
//     "Repository": "RSPM",
//     "Requirements": [
//     "R"
//     ],
//     "Hash": "470851b6d5d0ac559e9d01bb352b4021"
// },
fn resolve_repository<'a>(
    pkg_info: &'a PackageInfo,
    repositories: &'a [RenvRepository],
    repository_database: &[(RepositoryDatabase, bool)],
    r_version: &Version,
) -> Result<Source<'a>, Box<dyn Error>> {
    // For simplicity, if a package has fields RemoteUrl and RemoteSha, the package will be treated like it is from Git, even though it has source Repository
    // This is often the case for R-Universe, where it uses Git instead of an archive https://ropensci.org/blog/2022/01/06/runiverse-renv/
    // Expected RUniverse source package format from renv.lock:
    // "dvs": {
    //     "Package": "dvs",
    //     "Version": "0.0.2.9000",
    //     "Source": "Repository",
    //     "Repository": "https://a2-ai.r-universe.dev",
    //     "RemoteUrl": "https://github.com/a2-ai/dvs",
    //     "RemoteSha": "02c7ca5614a1f94acb5f2770b11dede062b1de63",
    //     "Requirements": [
    //       "rlang"
    //     ],
    //     "Hash": "13b178e8a0308dede915de93018ab60a"
    //   },
    if let (Some(git), Some(sha)) = (&pkg_info.remote_url, &pkg_info.remote_sha) {
        return Ok(Source::Git {
            git: git.to_string(),
            sha,
            directory: pkg_info.remote_subdir.as_deref(),
        });
    }

    let version_requirement = VersionRequirement::new(pkg_info.version.clone(), Operator::Equal);

    // match the repository database with its corresponding repository
    let repo_pairs = repositories
        .iter()
        .zip(repository_database)
        .map(|(repo, (repo_db, force_source))| (repo, repo_db, force_source))
        .collect::<Vec<_>>();

    // if a repository is found as that is specified by the package log, look in it first
    let pref_repo_pair = pkg_info
        .repository
        .as_ref()
        .and_then(|repo_name| repo_pairs.iter().find(|(r, _, _)| &r.name == repo_name));
    if let Some((repo, repo_db, force_source)) = pref_repo_pair
        && repo_db
            .find_package(
                &pkg_info.package,
                Some(&version_requirement),
                r_version,
                **force_source,
            )
            .is_some()
    {
        return Ok(Source::Repository(repo));
    };

    // if a repository is not found in its specified repository, look in the rest of the repositories
    // sacrificing one additional iteration step of re-looking up in preferred repository for less complexity
    if let Some((found_pkg, repo)) =
        repo_pairs
            .into_iter()
            .find_map(|(repo, repo_db, force_source)| {
                let (pkg, _) =
                    repo_db.find_package(&pkg_info.package, None, r_version, *force_source)?;
                Some((pkg, repo))
            })
    {
        if found_pkg.version == pkg_info.version {
            Ok(Source::Repository(repo))
        } else {
            Err(format!(
                "Package version ({}) not found in repositories. Found version {} in {}",
                pkg_info.version, found_pkg.version, repo.url
            )
            .into())
        }
    } else {
        Err("Package not found in repositories".into())
    }
}

// Expected generic git sourced package format from renv.lock
// "mypkg": {
//     "Package": "mypkg",
//     "Version": "0.1.0",
//     "Source": "Git",
//     "RemoteType": "git",
//     "RemoteUrl": "https://gitlab.com/org/mypkg.git",
//     "RemoteSha": "0123456789abcdef",
// }
fn resolve_git(pkg_info: &PackageInfo) -> Result<Source<'_>, Box<dyn Error>> {
    let git = pkg_info.remote_url.as_ref().ok_or("RemoteUrl not found")?;
    let sha = &pkg_info.remote_sha.as_ref().ok_or("RemoteSha not found")?;
    let directory = pkg_info.remote_subdir.as_deref();
    Ok(Source::Git {
        git: git.to_string(),
        sha,
        directory,
    })
}

// Expected GitHub sourced package format from renv.lock
// "ghqc": {
//     "Package": "ghqc",
//     "Version": "0.3.2",
//     "Source": "GitHub",
//     "RemoteType": "github",
//     "RemoteHost": "api.github.com",
//     "RemoteRepo": "ghqc",
//     "RemoteUsername": "a2-ai",
//     "RemoteSha": "55c23eb6a444542dab742d3d37c7b65af7b12e38",
//     "Requirements": [
//       "R",
//       "cli",
//       "fs",
//       "glue",
//       "httpuv",
//       "rlang",
//       "rstudioapi",
//       "withr",
//       "yaml"
//     ],
fn resolve_github(pkg_info: &PackageInfo) -> Result<Source<'_>, Box<dyn Error>> {
    let host = pkg_info
        .remote_host
        .as_ref()
        .ok_or("RemoteHost not found")?;
    let repo = pkg_info
        .remote_repo
        .as_ref()
        .ok_or("RemoteRepo not found")?;
    let org = &pkg_info
        .remote_username
        .as_ref()
        .ok_or("RemoteUsername not found")?;
    let sha = &pkg_info.remote_sha.as_ref().ok_or("RemoteSha not found")?;
    let directory = pkg_info.remote_subdir.as_deref();
    let base_url = host
        .trim_start_matches("https://")
        .trim_start_matches("api.")
        .trim_end_matches("api/v3");
    let git = format!("https://{base_url}/{org}/{repo}");
    Ok(Source::Git {
        git,
        sha,
        directory,
    })
}

// Expected local sourced package, installed via renv::install, format from renv.lock
// "rv.git.pkgA": {
//       "Package": "rv.git.pkgA",
//       "Version": "0.0.0.9000",
//       "Source": "Local",
//       "RemoteType": "local",
//       "RemoteUrl": "~/projects/rv.git.pkgA_0.0.0.9000.tar.gz",
//       "Hash": "39e317a9ec5437bd5ce021ad56da04b6"
//     }
fn resolve_local(pkg_info: &PackageInfo) -> Result<Source<'_>, Box<dyn Error>> {
    let path = pkg_info.remote_url.as_ref().ok_or("RemoteUrl not found")?;
    Ok(Source::Local(PathBuf::from(path)))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedRenv<'a> {
    package_info: &'a PackageInfo,
    source: Source<'a>,
}

impl fmt::Display for ResolvedRenv<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = &self.package_info.package;
        match &self.source {
            Source::Repository(r) => {
                write!(f, r#"{{ name = "{name}", repository = "{}" }}"#, r.name)
            }
            Source::Git {
                git,
                sha,
                directory,
            } => {
                write!(
                    f,
                    r#"{{ name = "{name}", git = "{git}", commit = "{sha}"{} }}"#,
                    directory
                        .as_ref()
                        .map(|d| format!(", directory = {d}"))
                        .unwrap_or_default()
                )
            }
            Source::Local(path) => {
                write!(f, r#"{{ name = "{name}", path = "{}" }}"#, path.display())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Source<'a> {
    Repository(&'a RenvRepository),
    Git {
        git: String,
        sha: &'a str,
        directory: Option<&'a str>,
    },
    Local(PathBuf),
}

pub struct UnresolvedRenv {
    package_info: PackageInfo,
    error: Box<dyn Error>,
}

impl fmt::Display for UnresolvedRenv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` could not be resolved due to: {:?}",
            self.package_info.package, self.error
        )
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum FromJsonFileErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] serde_json::Error),
}

#[derive(Debug, thiserror::Error)]
#[error("Error reading `{path}`")]
#[non_exhaustive]
pub struct FromJsonFileError {
    pub path: Box<Path>,
    pub source: FromJsonFileErrorKind,
}

/// Attempts to parse a GitHub git URL into (username, repo).
/// Supports both HTTP ("https://github.com/a2-ai/ghqc") and SSH ("git@github.com:a2-ai/ghqc.git").
fn parse_github_url(git_url: &GitUrl) -> Option<(String, String)> {
    match git_url {
        GitUrl::Http(url) => {
            if url.host_str() != Some("github.com") {
                return None;
            }
            let segments: Vec<_> = url.path_segments()?.collect();
            if segments.len() >= 2 {
                let repo = segments[1].trim_end_matches(".git");
                Some((segments[0].to_string(), repo.to_string()))
            } else {
                None
            }
        }
        GitUrl::Ssh(url) => {
            let rest = url.strip_prefix("git@github.com:")?;
            let rest = rest.trim_end_matches(".git");
            let (org, repo) = rest.split_once('/')?;
            Some((org.to_string(), repo.to_string()))
        }
    }
}

fn locked_package_to_renv(
    pkg: &crate::LockedPackage,
    url_to_alias: &HashMap<&str, &str>,
) -> Option<(PackageInfo, Option<String>)> {
    let mut warning = None;

    let version = Version::from_str(&pkg.version).ok()?;

    let requirements: Vec<String> = pkg
        .dependencies
        .iter()
        .map(|d| d.name())
        .filter(|name| *name != "R" && !BASE_PACKAGES.contains(name))
        .map(|s| s.to_string())
        .collect();

    let mut info = PackageInfo {
        package: pkg.name.clone(),
        version,
        requirements,
        // default, overridden below
        source: RenvSource::Repository,
        repository: None,
        remote_type: None,
        remote_host: None,
        remote_repo: None,
        remote_username: None,
        remote_sha: None,
        remote_subdir: None,
        remote_url: None,
        hash: None,
    };

    match &pkg.source {
        LockSource::Repository { repository } => {
            info.source = RenvSource::Repository;
            info.repository = Some(
                url_to_alias
                    .get(repository.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| repository.to_string()),
            );
        }
        LockSource::Git {
            git,
            sha,
            directory,
            ..
        } => {
            if let Some((username, repo)) = parse_github_url(git) {
                info.source = RenvSource::GitHub;
                info.remote_type = Some("github".into());
                info.remote_host = Some("api.github.com".into());
                info.remote_username = Some(username);
                info.remote_repo = Some(repo);
                info.remote_sha = Some(sha.clone());
                info.remote_subdir = directory.clone();
            } else {
                info.source = RenvSource::Git;
                info.remote_type = Some("git".into());
                info.remote_url = Some(git.url().to_string());
                info.remote_sha = Some(sha.clone());
                info.remote_subdir = directory.clone();
            }
        }
        LockSource::RUniverse {
            repository,
            git,
            sha,
            directory,
        } => {
            info.source = RenvSource::Repository;
            info.repository = Some(
                url_to_alias
                    .get(repository.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| repository.to_string()),
            );
            info.remote_url = Some(git.url().to_string());
            info.remote_sha = Some(sha.clone());
            info.remote_subdir = directory.clone();
        }
        LockSource::Local { path, .. } => {
            info.source = RenvSource::Local;
            info.remote_type = Some("local".into());
            info.remote_url = Some(path.display().to_string());
        }
        LockSource::Url { url, sha } => {
            info.source = RenvSource::Repository;
            info.remote_type = Some("url".into());
            info.remote_url = Some(url.as_str().to_string());
            info.remote_sha = Some(sha.clone());
            warning = Some(format!(
                "URL source `{url}` has no direct renv equivalent, mapped as Repository with RemoteUrl"
            ));
        }
        LockSource::Builtin { .. } => return None,
    }

    Some((info, warning))
}

fn normalize_renv_r_version(r_version: &str) -> String {
    if r_version.matches('.').count() < 2 {
        format!("{r_version}.0")
    } else {
        r_version.to_string()
    }
}

/// Convert an rv Lockfile + Config into an RenvLock.
/// Returns the RenvLock and a list of warnings for packages that couldn't be perfectly mapped.
pub fn to_renv_lock(lockfile: &Lockfile, config: &Config) -> (RenvLock, Vec<String>) {
    let mut warnings = Vec::new();

    let r_version = Version::from_str(&normalize_renv_r_version(lockfile.r_version_string()))
        .expect("valid R version in lockfile");

    let repositories: Vec<RenvRepository> = config
        .repositories()
        .iter()
        .map(|r| RenvRepository {
            name: r.alias.clone(),
            url: r.url().to_string(),
        })
        .collect();

    let url_to_alias: HashMap<&str, &str> = config
        .repositories()
        .iter()
        .map(|r| (r.url(), r.alias.as_str()))
        .collect();

    let mut packages = BTreeMap::new();
    for pkg in lockfile.packages() {
        if let Some((info, warning)) = locked_package_to_renv(pkg, &url_to_alias) {
            if let Some(w) = warning {
                warnings.push(format!("`{}`: {w}", pkg.name));
            }
            packages.insert(pkg.name.clone(), info);
        }
    }

    let renv_lock = RenvLock {
        r: RInfo {
            version: r_version,
            repositories,
        },
        packages,
    };

    (renv_lock, warnings)
}

#[cfg(test)]
mod tests {
    use crate::{Config, Lockfile, Repository, RepositoryDatabase, Version};

    use super::{RenvLock, to_renv_lock};

    fn repository_databases(
        r_version: &Version,
        repositories: &[Repository],
    ) -> Vec<(RepositoryDatabase, bool)> {
        let mut res = Vec::new();

        for r in repositories {
            let mut repo = RepositoryDatabase::new(r.url.as_str());
            let path = format!("src/tests/package_files/{}.PACKAGE", &r.alias);
            let text = std::fs::read_to_string(path).unwrap();
            if r.alias.contains("binary") {
                repo.parse_binary(&text, r_version.major_minor());
            } else {
                repo.parse_source(&text);
            }
            res.push((repo, false));
        }

        res
    }

    #[test]
    fn test_renv_lock_parse() {
        let renv_lock = RenvLock::parse_renv_lock("src/tests/renv/renv.lock").unwrap();
        let repository_databases =
            repository_databases(renv_lock.r_version(), &renv_lock.config_repositories());
        let (resolved, unresolved) = renv_lock.resolve(&repository_databases);

        let mut out = String::new();
        for r in resolved {
            out.push_str(&format!("{r}\n"));
        }

        out.push_str("--- unresolved --- \n");
        for u in unresolved {
            out.push_str(&format!("{u}\n"));
        }

        insta::assert_snapshot!("renv_resolver".to_string(), out);
    }

    #[test]
    fn test_renv_export() {
        let lockfile_toml = r#"
version = 2
r_version = "4.4.2"

[[packages]]
name = "rlang"
version = "1.1.4"
source = { repository = "https://cran.r-project.org/" }
force_source = false
dependencies = []

[[packages]]
name = "cli"
version = "3.6.3"
source = { repository = "https://cran.r-project.org/" }
force_source = false
dependencies = ["rlang"]

[[packages]]
name = "ghqc"
version = "0.3.2"
source = { git = "https://github.com/a2-ai/ghqc", sha = "55c23eb6a444542dab742d3d37c7b65af7b12e38" }
force_source = false
dependencies = ["cli", "rlang"]

[[packages]]
name = "gitlabpkg"
version = "0.1.0"
source = { git = "https://gitlab.com/a2-ai/gitlabpkg.git", sha = "0123456789abcdef0123456789abcdef01234567" }
force_source = false
dependencies = []

[[packages]]
name = "localpkg"
version = "0.1.0"
source = { path = "/tmp/localpkg" }
force_source = false
dependencies = []
"#;

        let config_toml = r#"
[project]
name = "test"
r_version = "4.4"
repositories = [
    { alias = "CRAN", url = "https://cran.r-project.org/" },
]
dependencies = ["rlang", "cli"]
"#;

        let lockfile: Lockfile = lockfile_toml.parse().unwrap();
        let config: Config = config_toml.parse().unwrap();
        let (renv_lock, warnings) = to_renv_lock(&lockfile, &config);

        let out = serde_json::to_string_pretty(&renv_lock).unwrap();
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        insta::assert_snapshot!("renv_export", out);
    }

    #[test]
    fn test_renv_parse_generic_git_source() {
        let renv_lock: RenvLock = serde_json::from_str(
            r#"
{
  "R": {
    "Version": "4.4.1",
    "Repositories": []
  },
  "Packages": {
    "gitlabpkg": {
      "Package": "gitlabpkg",
      "Version": "0.1.0",
      "Source": "Git",
      "RemoteType": "git",
      "RemoteUrl": "https://gitlab.com/a2-ai/gitlabpkg.git",
      "RemoteSha": "0123456789abcdef0123456789abcdef01234567"
    }
  }
}
"#,
        )
        .unwrap();

        let (resolved, unresolved) = renv_lock.resolve(&[]);

        assert!(unresolved.is_empty(), "unexpected unresolved entries");
        assert_eq!(
            resolved
                .into_iter()
                .map(|entry| entry.to_string())
                .collect::<Vec<_>>(),
            vec![
                r#"{ name = "gitlabpkg", git = "https://gitlab.com/a2-ai/gitlabpkg.git", commit = "0123456789abcdef0123456789abcdef01234567" }"#.to_string()
            ]
        );
    }
}
