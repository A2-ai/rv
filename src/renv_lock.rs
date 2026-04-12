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
//! let renv_lock = generate_renv_lock(&lockfile, &config, library_path, &[]).unwrap();
//! let json = serde_json::to_string_pretty(&renv_lock).unwrap();
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;
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

/// Compute the set of packages to exclude from renv.lock output.
///
/// Each name in `exclude_names` must be a top-level dependency in the config.
/// Returns the full set of package names to exclude (the excluded packages plus
/// any transitive dependencies that are only needed by excluded packages).
///
/// Errors if:
/// - An excluded package is not a top-level dependency in rproject.toml
/// - An excluded package is required by a non-excluded package's dependency tree
pub fn compute_exclusion_set<'a>(
    lockfile: &'a Lockfile,
    config: &Config,
    exclude_names: &[String],
) -> Result<HashSet<&'a str>> {
    if exclude_names.is_empty() {
        return Ok(HashSet::new());
    }

    let top_level_names: HashSet<&str> = config.dependencies().iter().map(|d| d.name()).collect();

    // Validate all excluded names are top-level dependencies
    for name in exclude_names {
        if !top_level_names.contains(name.as_str()) {
            bail!(
                "Cannot exclude '{}': it is not a top-level dependency in rproject.toml.\n\
                 Only packages listed in [project] dependencies can be excluded.",
                name
            );
        }
    }

    let exclude_set: HashSet<&str> = exclude_names.iter().map(|s| s.as_str()).collect();

    // Compute retained set: union of dep trees for all non-excluded top-level deps
    let mut retained_set: HashSet<&str> = HashSet::new();
    for dep in config.dependencies() {
        if !exclude_set.contains(dep.name()) {
            retained_set.extend(lockfile.get_package_tree(dep.name(), Some(dep)));
        }
    }

    // Compute excluded candidates: union of dep trees for excluded packages
    let mut excluded_candidates: HashSet<&str> = HashSet::new();
    for name in exclude_names {
        excluded_candidates.extend(lockfile.get_package_tree(name, None));
    }

    // Packages to actually exclude = candidates not retained by other packages
    let exclusion_set: HashSet<&str> = excluded_candidates
        .difference(&retained_set)
        .copied()
        .collect();

    // Validate: each explicitly excluded package must actually be excludable
    // (if it's in retained_set, a non-excluded package depends on it)
    for name in exclude_names {
        if !exclusion_set.contains(name.as_str()) {
            // Find which non-excluded top-level dep requires it
            let required_by: Vec<&str> = config
                .dependencies()
                .iter()
                .filter(|d| !exclude_set.contains(d.name()))
                .filter(|d| {
                    lockfile
                        .get_package_tree(d.name(), Some(d))
                        .contains(name.as_str())
                })
                .map(|d| d.name())
                .collect();
            bail!(
                "Cannot exclude '{}': it is required by non-excluded package(s): {}",
                name,
                required_by.join(", ")
            );
        }
    }

    Ok(exclusion_set)
}

/// Report describing the impact of package exclusions.
pub struct ExclusionReport {
    /// Packages directly requested for exclusion
    pub directly_excluded: Vec<String>,
    /// Transitive deps removed because they're only needed by excluded packages
    pub transitively_removed: Vec<String>,
    /// Deps of excluded packages that are kept because other packages need them
    pub retained: Vec<(String, Vec<String>)>,
    /// Total packages that would remain
    pub remaining_count: usize,
}

impl ExclusionReport {
    pub fn to_json(&self) -> Value {
        json!({
            "directly_excluded": self.directly_excluded,
            "transitively_removed": self.transitively_removed,
            "retained": self.retained.iter().map(|(pkg, required_by)| {
                json!({"package": pkg, "required_by": required_by})
            }).collect::<Vec<_>>(),
            "excluded_count": self.directly_excluded.len() + self.transitively_removed.len(),
            "remaining_count": self.remaining_count,
        })
    }
}

impl fmt::Display for ExclusionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Packages to exclude from renv.lock:\n")?;

        writeln!(f, "  Directly excluded (from --exclude-pkgs):")?;
        for pkg in &self.directly_excluded {
            writeln!(f, "    {pkg}")?;
        }

        if !self.transitively_removed.is_empty() {
            writeln!(
                f,
                "\n  Transitively removed (only needed by excluded packages):"
            )?;
            for pkg in &self.transitively_removed {
                writeln!(f, "    {pkg}")?;
            }
        }

        if !self.retained.is_empty() {
            writeln!(
                f,
                "\n  Retained despite being dependencies of excluded packages:"
            )?;
            for (pkg, required_by) in &self.retained {
                writeln!(f, "    {pkg} (also required by: {})", required_by.join(", "))?;
            }
        }

        let excluded_count = self.directly_excluded.len() + self.transitively_removed.len();
        writeln!(
            f,
            "\nTotal: {excluded_count} package(s) would be excluded, {} package(s) would remain",
            self.remaining_count
        )?;
        Ok(())
    }
}

/// Compute a detailed report of what would be excluded from renv.lock.
pub fn compute_exclusion_report(
    lockfile: &Lockfile,
    config: &Config,
    exclude_names: &[String],
) -> Result<ExclusionReport> {
    let exclusion_set = compute_exclusion_set(lockfile, config, exclude_names)?;
    let exclude_input: HashSet<&str> = exclude_names.iter().map(|s| s.as_str()).collect();

    let mut directly_excluded: Vec<String> = exclude_names.to_vec();
    directly_excluded.sort();

    let mut transitively_removed: Vec<String> = exclusion_set
        .iter()
        .filter(|name| !exclude_input.contains(*name))
        .map(|s| s.to_string())
        .collect();
    transitively_removed.sort();

    // Find retained deps: packages in excluded dep trees that aren't being removed
    let mut excluded_candidates: HashSet<&str> = HashSet::new();
    for name in exclude_names {
        excluded_candidates.extend(lockfile.get_package_tree(name, None));
    }
    let retained_names: HashSet<&&str> = excluded_candidates
        .iter()
        .filter(|name| !exclusion_set.contains(*name))
        .collect();

    let exclude_set: HashSet<&str> = exclude_names.iter().map(|s| s.as_str()).collect();
    let mut retained: Vec<(String, Vec<String>)> = retained_names
        .iter()
        .map(|&&pkg| {
            let required_by: Vec<String> = config
                .dependencies()
                .iter()
                .filter(|d| !exclude_set.contains(d.name()))
                .filter(|d| {
                    lockfile
                        .get_package_tree(d.name(), Some(d))
                        .contains(pkg)
                })
                .map(|d| d.name().to_string())
                .collect();
            (pkg.to_string(), required_by)
        })
        .collect();
    retained.sort_by(|a, b| a.0.cmp(&b.0));

    let total_non_builtin = lockfile
        .packages()
        .iter()
        .filter(|p| !p.source.is_builtin())
        .count();
    let remaining_count = total_non_builtin - exclusion_set.len();

    Ok(ExclusionReport {
        directly_excluded,
        transitively_removed,
        retained,
        remaining_count,
    })
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
    exclude_pkgs: &[String],
) -> Result<Value> {
    let exclusion_set = compute_exclusion_set(lockfile, config, exclude_pkgs)?;

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
        if exclusion_set.contains(locked_pkg.name.as_str()) {
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

    fn make_repo_locked_pkg(name: &str, repo_url: &str) -> LockedPackage {
        LockedPackage {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            source: Source::Repository {
                repository: url::Url::parse(repo_url).unwrap(),
            },
            path: None,
            force_source: false,
            dependencies: vec![],
            suggests: vec![],
        }
    }

    #[test]
    fn test_repository_field_overrides_description() {
        // The server stamps DESCRIPTION with `Repository: RSPM`, but the user's
        // rproject.toml maps that URL to alias "CRAN". The renv.lock output must
        // use the config alias, not the server-stamped value.
        let pkg = make_repo_locked_pkg(
            "R6",
            "https://packagemanager.posit.co/cran/2025-01-01/",
        );
        let desc = "Package: R6\nVersion: 1.0.0\nRepository: RSPM\n";
        let url_to_alias: HashMap<&str, &str> = HashMap::from([(
            "https://packagemanager.posit.co/cran/2025-01-01/",
            "CRAN",
        )]);

        let entry = package_to_renv_entry(&pkg, desc, &url_to_alias).unwrap();

        assert_eq!(entry["Source"], "Repository");
        // Must be the config alias "CRAN", not the server-stamped "RSPM"
        assert_eq!(entry["Repository"], "CRAN");
        assert_ne!(
            entry["Repository"], "RSPM",
            "Repository field must not leak the server-stamped value"
        );
    }

    #[test]
    fn test_repository_field_without_description_repository() {
        // DESCRIPTION has no Repository field at all (e.g., a custom/internal repo).
        // The output must still get the correct alias from the config.
        let pkg = make_repo_locked_pkg(
            "custompkg",
            "https://internal.example.com/repo/",
        );
        let desc = "Package: custompkg\nVersion: 1.0.0\nTitle: Custom Package\n";
        let url_to_alias: HashMap<&str, &str> =
            HashMap::from([("https://internal.example.com/repo/", "Internal")]);

        let entry = package_to_renv_entry(&pkg, desc, &url_to_alias).unwrap();

        assert_eq!(entry["Source"], "Repository");
        assert_eq!(entry["Repository"], "Internal");
    }

    // --- Exclusion set tests ---
    //
    // These use temp files to construct Lockfile + Config since their fields are private.
    // Dependency graph for tests:
    //
    //   top-level: pkgA, pkgB, devpkg
    //   pkgA   → sharedlib, utilA
    //   pkgB   → sharedlib
    //   devpkg → devhelper, sharedlib
    //
    // Excluding devpkg should remove devpkg + devhelper, but keep sharedlib (needed by pkgA, pkgB).

    fn make_exclusion_test_fixtures() -> (tempfile::TempDir, Lockfile, Config) {
        let dir = tempfile::TempDir::new().unwrap();

        let config_content = r#"[project]
name = "test-exclusion"
r_version = "4.5"
repositories = [
    { alias = "CRAN", url = "https://cran.example.com/" }
]
dependencies = ["pkgA", "pkgB", "devpkg"]
"#;
        let config_path = dir.path().join("rproject.toml");
        std::fs::write(&config_path, config_content).unwrap();

        let lockfile_content = r#"version = 2
r_version = "4.5"

[[packages]]
name = "pkgA"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = ["sharedlib", "utilA"]

[[packages]]
name = "pkgB"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = ["sharedlib"]

[[packages]]
name = "devpkg"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = ["devhelper", "sharedlib"]

[[packages]]
name = "sharedlib"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = []

[[packages]]
name = "utilA"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = []

[[packages]]
name = "devhelper"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = []
"#;
        let lockfile_path = dir.path().join("rv.lock");
        std::fs::write(&lockfile_path, lockfile_content).unwrap();

        let config = Config::from_file(&config_path).unwrap();
        let lockfile = Lockfile::load(&lockfile_path).unwrap().unwrap();

        (dir, lockfile, config)
    }

    #[test]
    fn test_exclusion_empty_list() {
        let (_dir, lockfile, config) = make_exclusion_test_fixtures();
        let result = compute_exclusion_set(&lockfile, &config, &[]).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_exclusion_removes_pkg_and_exclusive_deps() {
        let (_dir, lockfile, config) = make_exclusion_test_fixtures();
        let excluded = vec!["devpkg".to_string()];
        let result = compute_exclusion_set(&lockfile, &config, &excluded).unwrap();

        // devpkg and devhelper should be excluded
        assert!(result.contains("devpkg"));
        assert!(result.contains("devhelper"));
        // sharedlib is used by pkgA and pkgB — must NOT be excluded
        assert!(!result.contains("sharedlib"));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_exclusion_keeps_shared_deps() {
        let (_dir, lockfile, config) = make_exclusion_test_fixtures();
        let excluded = vec!["pkgA".to_string()];
        let result = compute_exclusion_set(&lockfile, &config, &excluded).unwrap();

        // pkgA and utilA (exclusive to pkgA) should be excluded
        assert!(result.contains("pkgA"));
        assert!(result.contains("utilA"));
        // sharedlib is used by pkgB and devpkg — must NOT be excluded
        assert!(!result.contains("sharedlib"));
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_exclusion_multiple_packages() {
        let (_dir, lockfile, config) = make_exclusion_test_fixtures();
        let excluded = vec!["pkgA".to_string(), "devpkg".to_string()];
        let result = compute_exclusion_set(&lockfile, &config, &excluded).unwrap();

        // pkgA, utilA, devpkg, devhelper should be excluded
        assert!(result.contains("pkgA"));
        assert!(result.contains("utilA"));
        assert!(result.contains("devpkg"));
        assert!(result.contains("devhelper"));
        // sharedlib still needed by pkgB
        assert!(!result.contains("sharedlib"));
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn test_exclusion_rejects_non_top_level() {
        let (_dir, lockfile, config) = make_exclusion_test_fixtures();
        let excluded = vec!["devhelper".to_string()];
        let result = compute_exclusion_set(&lockfile, &config, &excluded);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not a top-level dependency"));
    }

    #[test]
    fn test_exclusion_rejects_required_by_other() {
        // Create a scenario where pkgB depends on devpkg
        let dir = tempfile::TempDir::new().unwrap();
        let config_content = r#"[project]
name = "test"
r_version = "4.5"
repositories = [{ alias = "CRAN", url = "https://cran.example.com/" }]
dependencies = ["pkgA", "devpkg"]
"#;
        let lockfile_content = r#"version = 2
r_version = "4.5"

[[packages]]
name = "pkgA"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = ["devpkg"]

[[packages]]
name = "devpkg"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = []
"#;
        std::fs::write(dir.path().join("rproject.toml"), config_content).unwrap();
        std::fs::write(dir.path().join("rv.lock"), lockfile_content).unwrap();

        let config = Config::from_file(dir.path().join("rproject.toml")).unwrap();
        let lockfile = Lockfile::load(dir.path().join("rv.lock")).unwrap().unwrap();

        let excluded = vec!["devpkg".to_string()];
        let result = compute_exclusion_set(&lockfile, &config, &excluded);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("required by non-excluded package(s)"));
        assert!(err.contains("pkgA"));
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
