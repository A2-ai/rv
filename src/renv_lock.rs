//! Generate renv.lock files from rv lockfile data and installed library DESCRIPTION files.
//!
//! The main entry point is [`generate_renv_lock`], which reads an rv [`Lockfile`],
//! project [`Config`], and the installed library path to produce a `serde_json::Value`
//! representing a complete renv.lock file.
//!
//! # Supported source types
//!
//! | rv source | renv.lock mapping |
//! |-----------|-------------------|
//! | `Repository` | `Source: "Repository"` with alias from config |
//! | `Git` (GitHub only) | `Source: "GitHub"` with `Remote*` fields |
//! | `RUniverse` | `Source: "Repository"` with `RemoteUrl`/`RemoteSha` |
//! | `Local` | `Source: "Local"` |
//! | `Builtin` | Excluded (base R packages) |
//! | `Url` | **Not supported** — returns an error |
//! | `Git` (non-GitHub) | **Not supported** — returns an error |
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use rv::{Config, Lockfile};
//! use rv::renv_lock::generate_renv_lock;
//!
//! let config = Config::from_file("rproject.toml").unwrap();
//! let lockfile = Lockfile::load("rv.lock").unwrap().unwrap();
//! let library_path = Path::new("rv/library/4.5/x86_64/noble");
//!
//! let renv_lock = generate_renv_lock(&lockfile, &config, library_path).unwrap();
//! let json = serde_json::to_string_pretty(&renv_lock).unwrap();
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

use crate::lockfile::{LockedPackage, Source};
use crate::package::description::parse_description_fields;
use crate::{Config, Lockfile};

/// Fields to exclude from the renv.lock output (present in DESCRIPTION but not in renv.lock)
const EXCLUDED_FIELDS: &[&str] = &[
    "Built",
    "Packaged",
    "Date/Publication",
    "MD5sum",
    "GithubHost",
    "GithubRepo",
    "GithubUsername",
    "GithubRef",
    "GithubSHA1",
    "GithubSubdir",
    // renv excludes these standard Remote* fields for Repository packages
    "RemoteType",
    "RemoteRef",
    "RemotePkgRef",
    "RemoteRepos",
    "RemoteReposName",
    "RemotePkgPlatform",
    "RemoteSha",
    "RemoteHost",
    "RemoteUsername",
    "RemoteRepo",
    "RemoteSubdir",
    "RemoteUrl",
];

/// Dependency-like fields that renv parses into JSON arrays
const ARRAY_FIELDS: &[&str] = &["Depends", "Imports", "Suggests", "Enhances", "LinkingTo"];

/// Parse a dependency string like "R (>= 3.6), dplyr, testthat (>= 3.0.0)"
/// into a vec: ["R (>= 3.6)", "dplyr", "testthat (>= 3.0.0)"]
fn parse_dep_field_to_array(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a git URL to extract (remote_host, username, repo).
/// For github.com, RemoteHost becomes "api.github.com".
fn parse_git_url(url: &str) -> Option<(String, String, String)> {
    // Handle HTTPS URLs
    if let Ok(parsed) = url::Url::parse(url) {
        let host = parsed.host_str()?;
        let segments: Vec<&str> = parsed.path().trim_matches('/').split('/').collect();
        if segments.len() >= 2 {
            let username = segments[0].to_string();
            let repo = segments[1].trim_end_matches(".git").to_string();
            let remote_host = if host == "github.com" {
                "api.github.com".to_string()
            } else {
                host.to_string()
            };
            return Some((remote_host, username, repo));
        }
    }

    // Handle SSH URLs like git@github.com:org/repo.git
    if let Some(rest) = url.strip_prefix("git@")
        && let Some((host, path)) = rest.split_once(':')
    {
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();
        if segments.len() >= 2 {
            let username = segments[0].to_string();
            let repo = segments[1].trim_end_matches(".git").to_string();
            let remote_host = if host == "github.com" {
                "api.github.com".to_string()
            } else {
                host.to_string()
            };
            return Some((remote_host, username, repo));
        }
    }

    None
}

fn is_github_host(host: &str) -> bool {
    host == "github.com" || host == "api.github.com"
}

/// Convert a LockedPackage + its DESCRIPTION content into an renv.lock JSON entry.
fn package_to_renv_entry(
    locked_pkg: &LockedPackage,
    description_content: &str,
    url_to_alias: &HashMap<&str, &str>,
) -> Result<Value> {
    let raw_fields = parse_description_fields(description_content);
    let mut map = Map::new();

    // Add all DESCRIPTION fields, converting dep fields to arrays and excluding renv internals
    for (key, value) in &raw_fields {
        if EXCLUDED_FIELDS.contains(&key.as_str()) {
            continue;
        }
        if ARRAY_FIELDS.contains(&key.as_str()) {
            let arr = parse_dep_field_to_array(value);
            if !arr.is_empty() {
                map.insert(key.clone(), json!(arr));
            }
        } else {
            map.insert(key.clone(), json!(value));
        }
    }

    // Inject/override Source-specific fields based on rv.lock
    match &locked_pkg.source {
        Source::Repository { repository } => {
            let repo_url = repository.as_str();
            let alias = url_to_alias.get(repo_url).copied().unwrap_or_else(|| {
                let trimmed = repo_url.trim_end_matches('/');
                url_to_alias.get(trimmed).copied().unwrap_or(repo_url)
            });
            map.insert("Source".to_string(), json!("Repository"));
            map.insert("Repository".to_string(), json!(alias));
        }
        Source::Git {
            git,
            sha,
            tag,
            branch,
            directory,
        } => {
            let (host, username, repo) = parse_git_url(git.url()).ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not parse git URL '{}' for package '{}'",
                    git.url(),
                    locked_pkg.name
                )
            })?;

            if !is_github_host(&host) {
                bail!(
                    "Package '{}' uses a non-GitHub git remote ({}).\n\
                     renv.lock only supports GitHub remotes. \
                     Non-GitHub git sources are not yet supported by `rv renv lock`.",
                    locked_pkg.name,
                    git.url()
                );
            }

            map.insert("Source".to_string(), json!("GitHub"));
            map.insert("RemoteType".to_string(), json!("github"));
            map.insert("RemoteHost".to_string(), json!(host));
            map.insert("RemoteUsername".to_string(), json!(username));
            map.insert("RemoteRepo".to_string(), json!(repo));

            let remote_ref = tag.as_deref().or(branch.as_deref()).unwrap_or(sha.as_str());
            map.insert("RemoteRef".to_string(), json!(remote_ref));
            map.insert("RemoteSha".to_string(), json!(sha));

            if let Some(dir) = directory {
                map.insert("RemoteSubdir".to_string(), json!(dir));
            }
        }
        Source::RUniverse {
            repository,
            git,
            sha,
            directory,
        } => {
            map.insert("Source".to_string(), json!("Repository"));
            let repo_url = repository.as_str();
            let alias = url_to_alias.get(repo_url).copied().unwrap_or(repo_url);
            map.insert("Repository".to_string(), json!(alias));
            map.insert("RemoteUrl".to_string(), json!(git.url()));
            map.insert("RemoteSha".to_string(), json!(sha));
            if let Some(dir) = directory {
                map.insert("RemoteSubdir".to_string(), json!(dir));
            }
        }
        Source::Local { path, .. } => {
            map.insert("Source".to_string(), json!("Local"));
            map.insert("RemoteType".to_string(), json!("local"));
            map.insert("RemoteUrl".to_string(), json!(path.display().to_string()));
        }
        Source::Url { url, .. } => {
            bail!(
                "Package '{}' uses a URL source ({}).\n\
                 URL sources have no renv.lock equivalent and are not supported by `rv renv lock`.",
                locked_pkg.name,
                url
            );
        }
        Source::Builtin { .. } => {
            // Should not reach here — builtins are filtered before this function
        }
    }

    Ok(Value::Object(map))
}

/// Generate a complete renv.lock JSON structure from an rv lockfile, project config,
/// and installed library.
///
/// Reads each package's DESCRIPTION file from `library_path` and combines it with
/// source metadata from the lockfile to produce renv-compatible output. Dependency
/// fields (`Depends`, `Imports`, `Suggests`, `Enhances`, `LinkingTo`) are parsed
/// into JSON arrays. Repository URLs are mapped to aliases using the project config.
///
/// Builtin (base R) packages are excluded. URL sources and non-GitHub git sources
/// will return an error.
///
/// The R version in the output uses the major.minor version from the rv lockfile.
pub fn generate_renv_lock(
    lockfile: &Lockfile,
    config: &Config,
    library_path: &Path,
) -> Result<Value> {
    let url_to_alias: HashMap<&str, &str> = config
        .repositories()
        .iter()
        .map(|r| (r.url(), r.alias.as_str()))
        .collect();

    let r_section = {
        let repos: Vec<Value> = config
            .repositories()
            .iter()
            .map(|r| json!({"Name": r.alias, "URL": r.url()}))
            .collect();
        json!({
            "Version": lockfile.r_version_str(),
            "Repositories": repos
        })
    };

    let mut packages = Map::new();

    for locked_pkg in lockfile.packages() {
        if locked_pkg.source.is_builtin() {
            continue;
        }

        let desc_path = library_path.join(&locked_pkg.name).join("DESCRIPTION");
        let description_content = std::fs::read_to_string(&desc_path).with_context(|| {
            format!(
                "Failed to read DESCRIPTION for package '{}' at {}. Is the library synced?",
                locked_pkg.name,
                desc_path.display()
            )
        })?;

        let entry = package_to_renv_entry(locked_pkg, &description_content, &url_to_alias)?;
        packages.insert(locked_pkg.name.clone(), entry);
    }

    Ok(json!({
        "R": r_section,
        "Packages": packages
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dep_field_to_array() {
        let deps = parse_dep_field_to_array("R (>= 3.6), dplyr, testthat (>= 3.0.0)");
        assert_eq!(deps, vec!["R (>= 3.6)", "dplyr", "testthat (>= 3.0.0)"]);
    }

    #[test]
    fn test_parse_dep_field_trailing_comma() {
        let deps = parse_dep_field_to_array("R (>= 3.6),");
        assert_eq!(deps, vec!["R (>= 3.6)"]);
    }

    #[test]
    fn test_parse_git_url_https_github() {
        let result = parse_git_url("https://github.com/A2-ai/rv.git.pkgA").unwrap();
        assert_eq!(result.0, "api.github.com");
        assert_eq!(result.1, "A2-ai");
        assert_eq!(result.2, "rv.git.pkgA");
    }

    #[test]
    fn test_parse_git_url_https_trailing_slash() {
        let result = parse_git_url("https://github.com/A2-ai/rv.git.pkgB/").unwrap();
        assert_eq!(result.0, "api.github.com");
        assert_eq!(result.1, "A2-ai");
        assert_eq!(result.2, "rv.git.pkgB");
    }

    #[test]
    fn test_parse_git_url_https_dot_git() {
        let result = parse_git_url("https://github.com/A2-ai/repo.git").unwrap();
        assert_eq!(result.2, "repo");
    }

    #[test]
    fn test_parse_git_url_ssh() {
        let result = parse_git_url("git@github.com:A2-ai/rv.git.pkgA.git").unwrap();
        assert_eq!(result.0, "api.github.com");
        assert_eq!(result.1, "A2-ai");
        assert_eq!(result.2, "rv.git.pkgA");
    }

    #[test]
    fn test_parse_git_url_non_github() {
        let result = parse_git_url("https://gitlab.com/org/repo").unwrap();
        assert_eq!(result.0, "gitlab.com");
        assert_eq!(result.1, "org");
        assert_eq!(result.2, "repo");
    }

    fn make_git_locked_pkg(tag: Option<&str>, branch: Option<&str>, sha: &str) -> LockedPackage {
        use crate::git::url::GitUrl;
        LockedPackage {
            name: "testpkg".to_string(),
            version: "1.0.0".to_string(),
            source: Source::Git {
                git: GitUrl::try_from("https://github.com/org/testpkg").unwrap(),
                sha: sha.to_string(),
                directory: None,
                tag: tag.map(|s| s.to_string()),
                branch: branch.map(|s| s.to_string()),
            },
            path: None,
            force_source: true,
            dependencies: vec![],
            suggests: vec![],
        }
    }

    const SIMPLE_DESC: &str = "Package: testpkg\nVersion: 1.0.0\nTitle: Test\n";

    #[test]
    fn test_git_tag_remote_ref() {
        let pkg = make_git_locked_pkg(Some("v1.0"), None, "abc123");
        let url_to_alias = HashMap::new();
        let entry = package_to_renv_entry(&pkg, SIMPLE_DESC, &url_to_alias).unwrap();
        assert_eq!(entry["Source"], "GitHub");
        assert_eq!(entry["RemoteRef"], "v1.0");
        assert_eq!(entry["RemoteSha"], "abc123");
        assert_eq!(entry["RemoteType"], "github");
    }

    #[test]
    fn test_git_branch_remote_ref() {
        let pkg = make_git_locked_pkg(None, Some("main"), "def456");
        let url_to_alias = HashMap::new();
        let entry = package_to_renv_entry(&pkg, SIMPLE_DESC, &url_to_alias).unwrap();
        assert_eq!(entry["Source"], "GitHub");
        assert_eq!(entry["RemoteRef"], "main");
        assert_eq!(entry["RemoteSha"], "def456");
    }

    #[test]
    fn test_git_commit_only_remote_ref() {
        let pkg = make_git_locked_pkg(None, None, "789abcdef0123456");
        let url_to_alias = HashMap::new();
        let entry = package_to_renv_entry(&pkg, SIMPLE_DESC, &url_to_alias).unwrap();
        assert_eq!(entry["Source"], "GitHub");
        assert_eq!(entry["RemoteRef"], "789abcdef0123456");
        assert_eq!(entry["RemoteSha"], "789abcdef0123456");
    }

    #[test]
    fn test_non_github_git_errors() {
        use crate::git::url::GitUrl;
        let pkg = LockedPackage {
            name: "gitlabpkg".to_string(),
            version: "1.0.0".to_string(),
            source: Source::Git {
                git: GitUrl::try_from("https://gitlab.com/org/repo").unwrap(),
                sha: "abc123".to_string(),
                directory: None,
                tag: None,
                branch: Some("main".to_string()),
            },
            path: None,
            force_source: true,
            dependencies: vec![],
            suggests: vec![],
        };
        let url_to_alias = HashMap::new();
        let result = package_to_renv_entry(&pkg, SIMPLE_DESC, &url_to_alias);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("non-GitHub git remote")
        );
    }

    #[test]
    fn test_url_source_errors() {
        let pkg = LockedPackage {
            name: "urlpkg".to_string(),
            version: "1.0.0".to_string(),
            source: Source::Url {
                url: url::Url::parse("https://example.com/pkg.tar.gz").unwrap(),
                sha: "abc123".to_string(),
            },
            path: None,
            force_source: true,
            dependencies: vec![],
            suggests: vec![],
        };
        let url_to_alias = HashMap::new();
        let result = package_to_renv_entry(&pkg, SIMPLE_DESC, &url_to_alias);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("URL sources have no renv.lock equivalent")
        );
    }
}
