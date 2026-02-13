use std::path::Path;

use std::fs;
use toml_edit::{Array, DocumentMut, Formatted, InlineTable, Value};

#[cfg(feature = "cli")]
use clap::Parser;

use crate::{Config, config::ConfigLoadError, git::url::GitUrl};

pub const DEFAULT_GIT_SHORTHAND_BASE_URL: &str = "https://github.com";
const DEFAULT_GIT_HEAD_REFERENCE: &str = "HEAD";

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "cli", derive(Parser))]
pub struct AddOptions {
    /// Pin package to a specific repository alias (must exist in config)
    #[cfg_attr(feature = "cli", clap(long, conflicts_with_all = ["git", "path", "url"]))]
    pub repository: Option<String>,
    /// Force building from source instead of using binaries
    #[cfg_attr(feature = "cli", clap(long, conflicts_with_all = ["git", "path", "url"]))]
    pub force_source: bool,
    /// Also install suggested packages
    #[cfg_attr(feature = "cli", clap(long))]
    pub install_suggestions: bool,
    /// Install only the dependencies, not the package itself
    #[cfg_attr(feature = "cli", clap(long))]
    pub dependencies_only: bool,
    /// Git repository URL (https or ssh)
    #[cfg_attr(feature = "cli", clap(long, conflicts_with_all = ["repository", "path", "url"]))]
    pub git: Option<String>,
    /// Git commit SHA
    #[cfg_attr(feature = "cli", clap(long, requires = "git", conflicts_with_all = ["tag", "branch"]))]
    pub commit: Option<String>,
    /// Git tag
    #[cfg_attr(feature = "cli", clap(long, requires = "git", conflicts_with_all = ["commit", "branch"]))]
    pub tag: Option<String>,
    /// Git branch
    #[cfg_attr(feature = "cli", clap(long, requires = "git", conflicts_with_all = ["commit", "tag"]))]
    pub branch: Option<String>,
    #[cfg_attr(feature = "cli", clap(skip))]
    /// Generic git reference (branch/tag/HEAD) used internally for shorthand specs
    pub reference: Option<String>,
    #[cfg_attr(feature = "cli", clap(long, requires = "git"))]
    /// Subdirectory within git repository
    pub directory: Option<String>,
    /// Local filesystem path to package directory or archive
    #[cfg_attr(feature = "cli", clap(long, conflicts_with_all = ["repository", "git", "url"]))]
    pub path: Option<String>,
    /// HTTP/HTTPS URL to package archive
    #[cfg_attr(feature = "cli", clap(long, conflicts_with_all = ["repository", "git", "path"]))]
    pub url: Option<String>,
}

impl AddOptions {
    pub fn has_details_options(&self) -> bool {
        self.repository.is_some()
            || self.force_source
            || self.install_suggestions
            || self.dependencies_only
            || self.git.is_some()
            || self.path.is_some()
            || self.url.is_some()
    }

    pub fn has_source_options(&self) -> bool {
        self.repository.is_some()
            || self.force_source
            || self.git.is_some()
            || self.path.is_some()
            || self.url.is_some()
    }

    pub fn is_empty(&self) -> bool {
        self == &Default::default()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedAddPackage {
    pub name: String,
    pub options: AddOptions,
}

pub fn read_and_verify_config(
    config_file: impl AsRef<Path>,
) -> Result<DocumentMut, DependencyEditError> {
    let config_file = config_file.as_ref();
    let _ = Config::from_file(config_file).map_err(|e| DependencyEditError {
        path: config_file.into(),
        source: Box::new(DependencyEditErrorKind::ConfigLoad(e)),
    })?;
    let config_content = fs::read_to_string(config_file).unwrap(); // Verified config could be loaded above

    Ok(config_content.parse::<DocumentMut>().unwrap()) // Verify config was valid toml above
}

pub fn parse_add_package_spec(
    package_spec: &str,
    git_shorthand_base_url: &str,
) -> Result<ParsedAddPackage, String> {
    if !looks_like_repo_spec(package_spec) {
        return Ok(ParsedAddPackage {
            name: package_spec.to_string(),
            options: AddOptions::default(),
        });
    }

    let is_git_url = is_git_url(package_spec);
    let (source_with_optional_directory, reference_part) =
        split_source_and_reference(package_spec, is_git_url);

    let (source, reference, directory) = if let Some(reference_part) = reference_part {
        let (reference, directory) = parse_reference_and_directory(reference_part.as_str())?;
        (source_with_optional_directory, Some(reference), directory)
    } else {
        let (source, directory) = split_source_and_directory(source_with_optional_directory)?;
        (source, None, directory)
    };

    let git_url = if is_git_url {
        source.clone()
    } else {
        resolve_shorthand_git_url(git_shorthand_base_url, source.as_str())?
    };

    GitUrl::try_from(git_url.as_str())
        .map_err(|e| format!("Invalid git URL `{git_url}` in spec `{package_spec}`: {e}"))?;

    let mut options = AddOptions {
        git: Some(git_url),
        directory,
        ..Default::default()
    };
    let package_name = extract_package_name(source.as_str())?;

    match reference {
        Some(ParsedReference::Commit(reference)) => options.commit = Some(reference),
        Some(ParsedReference::Tag(reference)) => options.tag = Some(reference),
        Some(ParsedReference::Branch(reference)) => options.branch = Some(reference),
        Some(ParsedReference::Unknown(reference)) => options.reference = Some(reference),
        None => options.reference = Some(DEFAULT_GIT_HEAD_REFERENCE.to_string()),
    }

    Ok(ParsedAddPackage {
        name: package_name,
        options,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedReference {
    Branch(String),
    Tag(String),
    Commit(String),
    Unknown(String),
}

fn looks_like_repo_spec(package_spec: &str) -> bool {
    if is_git_url(package_spec) {
        return true;
    }

    if package_spec.starts_with("./")
        || package_spec.starts_with("../")
        || package_spec.starts_with('/')
        || package_spec.starts_with("~/")
    {
        return false;
    }

    package_spec.contains('/')
}

fn is_git_url(package_spec: &str) -> bool {
    package_spec.starts_with("https://")
        || package_spec.starts_with("http://")
        || package_spec.starts_with("git@")
        || package_spec.starts_with("ssh@")
}

fn split_source_and_reference(package_spec: &str, is_git_url: bool) -> (String, Option<String>) {
    if !is_git_url {
        if let Some((source, reference)) = package_spec.split_once('@') {
            return (source.to_string(), Some(reference.to_string()));
        }

        return (package_spec.to_string(), None);
    }

    // URL-like inputs can contain `@` in userinfo, so only parse references from the path.
    if package_spec.starts_with("https://") || package_spec.starts_with("http://") {
        if let Some(path_start) = package_spec.find("://").and_then(|scheme_idx| {
            package_spec[scheme_idx + 3..]
                .find('/')
                .map(|i| scheme_idx + 3 + i)
        }) && let Some(at_idx) = package_spec[path_start..].rfind('@')
        {
            let at_idx = path_start + at_idx;
            return (
                package_spec[..at_idx].to_string(),
                Some(package_spec[at_idx + 1..].to_string()),
            );
        }
    } else if let Some(host_sep) = package_spec.find(':')
        && let Some(at_idx) = package_spec[host_sep + 1..].rfind('@')
    {
        let at_idx = host_sep + 1 + at_idx;
        return (
            package_spec[..at_idx].to_string(),
            Some(package_spec[at_idx + 1..].to_string()),
        );
    }

    (package_spec.to_string(), None)
}

fn split_source_and_directory(source: String) -> Result<(String, Option<String>), String> {
    if let Some(split_idx) = source.rfind(':') {
        let last_slash = source.rfind('/');
        if last_slash.is_some() && split_idx > last_slash.unwrap() {
            let directory = source[split_idx + 1..].trim();
            if directory.is_empty() {
                return Err(format!("Invalid repository subdirectory in `{source}`"));
            }
            return Ok((source[..split_idx].to_string(), Some(directory.to_string())));
        }
    }

    Ok((source, None))
}

fn parse_reference_and_directory(
    reference_part: &str,
) -> Result<(ParsedReference, Option<String>), String> {
    if reference_part.is_empty() {
        return Err("Missing git reference after `@`".to_string());
    }

    if let Some(raw_ref) = reference_part.strip_prefix("branch:") {
        let (reference, directory) = split_reference_and_directory(raw_ref)?;
        return Ok((ParsedReference::Branch(reference), directory));
    }

    if let Some(raw_ref) = reference_part.strip_prefix("tag:") {
        let (reference, directory) = split_reference_and_directory(raw_ref)?;
        return Ok((ParsedReference::Tag(reference), directory));
    }

    if let Some(raw_ref) = reference_part.strip_prefix("commit:") {
        let (reference, directory) = split_reference_and_directory(raw_ref)?;
        if !looks_like_commit_sha(reference.as_str()) {
            return Err(format!(
                "Invalid commit SHA `{reference}` in `@commit:<sha>` reference"
            ));
        }
        return Ok((ParsedReference::Commit(reference), directory));
    }

    let (reference, directory) = split_reference_and_directory(reference_part)?;
    if looks_like_commit_sha(reference.as_str()) {
        Ok((ParsedReference::Commit(reference), directory))
    } else {
        Ok((ParsedReference::Unknown(reference), directory))
    }
}

fn split_reference_and_directory(input: &str) -> Result<(String, Option<String>), String> {
    let (reference, directory) = if let Some((reference, directory)) = input.split_once(':') {
        (reference.trim(), Some(directory.trim().to_string()))
    } else {
        (input.trim(), None)
    };

    if reference.is_empty() {
        return Err(format!("Invalid empty git reference in `{input}`"));
    }

    if let Some(directory) = directory {
        if directory.is_empty() {
            return Err(format!("Invalid repository subdirectory in `{input}`"));
        }
        Ok((reference.to_string(), Some(directory)))
    } else {
        Ok((reference.to_string(), None))
    }
}

fn looks_like_commit_sha(reference: &str) -> bool {
    (7..=40).contains(&reference.len()) && reference.chars().all(|c| c.is_ascii_hexdigit())
}

fn resolve_shorthand_git_url(base_url: &str, source: &str) -> Result<String, String> {
    let trimmed_base = base_url.trim();
    if trimmed_base.is_empty() {
        return Err("Git shorthand base URL cannot be empty".to_string());
    }

    let source = source.trim().trim_start_matches('/');
    if source.is_empty() {
        return Err("Git shorthand source cannot be empty".to_string());
    }

    let full_url = if trimmed_base.ends_with(':') {
        format!("{trimmed_base}{source}")
    } else {
        format!("{}/{}", trimmed_base.trim_end_matches('/'), source)
    };

    Ok(full_url)
}

fn extract_package_name(source: &str) -> Result<String, String> {
    let source = source.trim_end_matches('/');

    let repo_part = if source.starts_with("https://") || source.starts_with("http://") {
        source
            .find("://")
            .and_then(|scheme_idx| {
                source[scheme_idx + 3..]
                    .find('/')
                    .map(|i| scheme_idx + 3 + i + 1)
            })
            .map(|path_start| source[path_start..].to_string())
    } else if source.starts_with("git@") || source.starts_with("ssh@") {
        source.split_once(':').map(|(_, path)| path.to_string())
    } else {
        Some(source.to_string())
    };

    let repo_part = repo_part.ok_or_else(|| format!("Invalid repository spec `{source}`"))?;
    let package_name = repo_part
        .split('/')
        .next_back()
        .map(|v| v.trim_end_matches(".git").to_string())
        .unwrap_or_default();

    if package_name.is_empty() {
        return Err(format!("Could not infer package name from `{source}`"));
    }

    Ok(package_name)
}

pub fn add_packages(
    config_doc: &mut DocumentMut,
    packages: Vec<String>,
    options: AddOptions,
) -> Result<(), DependencyEditError> {
    // get the dependencies array
    let config_deps = get_mut_array(config_doc);

    // collect the names of all of the dependencies
    let config_dep_names = config_deps
        .iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(s.value().as_str()),
            Value::InlineTable(t) => t.get("name").and_then(|v| v.as_str()),
            _ => None,
        })
        .map(|s| s.to_string()) // Need to allocate so values are not a reference to a mut
        .collect::<Vec<_>>();

    // Determine if the dep to add is in the config, if not add it
    for package_name in packages {
        if !config_dep_names.contains(&package_name) {
            let dep_value = create_dependency_value(&package_name, &options)?;
            config_deps.push(dep_value);
            // Couldn't format value before pushing, so adding formatting after its added
            if let Some(last) = config_deps.iter_mut().last() {
                last.decor_mut().set_prefix("\n    ");
            }
        }
    }

    // Set a trailing new line and comma for the last element for proper formatting
    config_deps.set_trailing("\n");
    config_deps.set_trailing_comma(true);

    Ok(())
}

fn create_dependency_value(
    package_name: &str,
    options: &AddOptions,
) -> Result<Value, DependencyEditError> {
    if options.is_empty() {
        // Simple string dependency
        return Ok(Value::String(Formatted::new(package_name.to_string())));
    }

    // Create an inline table for detailed dependencies
    let mut table = InlineTable::new();
    table.insert("name", Value::from(package_name));

    // Handle different dependency types
    if let Some(ref git_url) = options.git {
        // Git dependency
        table.insert("git", Value::from(git_url.as_str()));

        if let Some(ref commit) = options.commit {
            table.insert("commit", Value::from(commit.as_str()));
        } else if let Some(ref tag) = options.tag {
            table.insert("tag", Value::from(tag.as_str()));
        } else if let Some(ref branch) = options.branch {
            table.insert("branch", Value::from(branch.as_str()));
        } else if let Some(ref reference) = options.reference {
            table.insert("reference", Value::from(reference.as_str()));
        }

        if let Some(ref directory) = options.directory {
            table.insert("directory", Value::from(directory.as_str()));
        }
    } else if let Some(ref path) = options.path {
        // Local path dependency
        table.insert("path", Value::from(path.as_str()));
    } else if let Some(ref url) = options.url {
        // URL dependency
        table.insert("url", Value::from(url.as_str()));
    } else {
        // Detailed/repository dependency
        if let Some(ref repository) = options.repository {
            table.insert("repository", Value::from(repository.as_str()));
        }

        if options.force_source {
            table.insert("force_source", Value::from(true));
        }
    }

    // Add common options that apply to all dependency types
    add_common_options(&mut table, options);

    Ok(Value::InlineTable(table))
}

fn add_common_options(table: &mut InlineTable, options: &AddOptions) {
    if options.install_suggestions {
        table.insert("install_suggestions", Value::from(true));
    }

    if options.dependencies_only {
        table.insert("dependencies_only", Value::from(true));
    }
}

fn get_mut_array(doc: &mut DocumentMut) -> &mut Array {
    // the dependencies array is behind the project table
    let deps = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .unwrap()
        .entry("dependencies")
        .or_insert_with(|| Array::new().into())
        .as_array_mut()
        .unwrap();

    // remove formatting on the last element as we will re-add
    if let Some(last) = deps.iter_mut().last() {
        last.decor_mut().set_suffix("");
    }
    deps
}

pub fn remove_packages(
    config_doc: &mut DocumentMut,
    packages: Vec<String>,
) -> Result<(), DependencyEditError> {
    let config_deps = get_mut_array(config_doc);

    // Remove packages that match the names provided
    config_deps.retain(|v| {
        let dep_name = match v {
            Value::String(s) => s.value().as_str(),
            Value::InlineTable(t) => t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            _ => "",
        };

        // Keep the element if it doesn't match any package to remove
        !packages.iter().any(|p| p.as_str() == dep_name)
    });

    // Set a trailing new line and comma for the last element for proper formatting
    config_deps.set_trailing("\n");
    config_deps.set_trailing_comma(true);

    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to edit config at `{path}`")]
#[non_exhaustive]
pub struct DependencyEditError {
    path: Box<Path>,
    source: Box<DependencyEditErrorKind>,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum DependencyEditErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] toml_edit::TomlError),
    ConfigLoad(#[from] ConfigLoadError),
}

#[cfg(test)]
mod tests {
    use super::{AddOptions, DEFAULT_GIT_SHORTHAND_BASE_URL, parse_add_package_spec};
    use crate::{add_packages, read_and_verify_config, remove_packages};

    const BASELINE_ADD_CONFIG: &str = "src/tests/valid_config/baseline_for_add.toml";
    const BASELINE_REMOVE_CONFIG: &str = "src/tests/valid_config/baseline_for_remove.toml";

    // Simple tests - one feature at a time

    #[test]
    fn remove_package() {
        let mut doc = read_and_verify_config(BASELINE_REMOVE_CONFIG).unwrap();
        remove_packages(&mut doc, vec!["dplyr".to_string()]).unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_simple_package() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(&mut doc, vec!["dplyr".to_string()], AddOptions::default()).unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_with_repository() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["dplyr".to_string()],
            AddOptions {
                repository: Some("ppm".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_with_force_source() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["dplyr".to_string()],
            AddOptions {
                force_source: true,
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_with_install_suggestions() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["dplyr".to_string()],
            AddOptions {
                install_suggestions: true,
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_with_dependencies_only() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["dplyr".to_string()],
            AddOptions {
                dependencies_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_git_with_commit() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["mypkg".to_string()],
            AddOptions {
                git: Some("https://github.com/user/repo".to_string()),
                commit: Some("abc123def456".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_git_with_tag() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["mypkg".to_string()],
            AddOptions {
                git: Some("https://github.com/user/repo".to_string()),
                tag: Some("v1.0.0".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_git_with_branch() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["mypkg".to_string()],
            AddOptions {
                git: Some("https://github.com/user/repo".to_string()),
                branch: Some("main".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_git_with_directory() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["mypkg".to_string()],
            AddOptions {
                git: Some("https://github.com/user/repo".to_string()),
                branch: Some("main".to_string()),
                directory: Some("subdir".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_local_path() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["mypkg".to_string()],
            AddOptions {
                path: Some("../local/package".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_url() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["dplyr".to_string()],
            AddOptions {
                url: Some(
                    "https://cran.r-project.org/src/contrib/Archive/dplyr/dplyr_1.1.3.tar.gz"
                        .to_string(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn parse_simple_package_spec() {
        let parsed = parse_add_package_spec("dplyr", DEFAULT_GIT_SHORTHAND_BASE_URL).unwrap();
        assert_eq!(parsed.name, "dplyr");
        assert!(parsed.options.is_empty());
    }

    #[test]
    fn parse_owner_repo_defaults_to_head_reference() {
        let parsed = parse_add_package_spec("r-lib/cli", DEFAULT_GIT_SHORTHAND_BASE_URL).unwrap();
        assert_eq!(parsed.name, "cli");
        assert_eq!(
            parsed.options.git.as_deref(),
            Some("https://github.com/r-lib/cli")
        );
        assert_eq!(parsed.options.reference.as_deref(), Some("HEAD"));
        assert_eq!(parsed.options.directory, None);
    }

    #[test]
    fn parse_owner_repo_with_untyped_reference() {
        let parsed =
            parse_add_package_spec("r-lib/cli@v3.6.2", DEFAULT_GIT_SHORTHAND_BASE_URL).unwrap();
        assert_eq!(parsed.name, "cli");
        assert_eq!(parsed.options.reference.as_deref(), Some("v3.6.2"));
    }

    #[test]
    fn parse_owner_repo_with_typed_reference_and_directory() {
        let parsed = parse_add_package_spec(
            "r-lib/usethis@tag:v2.2.3:r-package",
            DEFAULT_GIT_SHORTHAND_BASE_URL,
        )
        .unwrap();
        assert_eq!(parsed.name, "usethis");
        assert_eq!(parsed.options.tag.as_deref(), Some("v2.2.3"));
        assert_eq!(parsed.options.directory.as_deref(), Some("r-package"));
    }

    #[test]
    fn parse_owner_repo_with_commit_sha() {
        let parsed =
            parse_add_package_spec("r-lib/rlang@9a8c5d2", DEFAULT_GIT_SHORTHAND_BASE_URL).unwrap();
        assert_eq!(parsed.name, "rlang");
        assert_eq!(parsed.options.commit.as_deref(), Some("9a8c5d2"));
    }

    #[test]
    fn parse_https_git_url_with_tag() {
        let parsed = parse_add_package_spec(
            "https://github.com/r-lib/cli.git@tag:v3.6.2",
            DEFAULT_GIT_SHORTHAND_BASE_URL,
        )
        .unwrap();
        assert_eq!(parsed.name, "cli");
        assert_eq!(
            parsed.options.git.as_deref(),
            Some("https://github.com/r-lib/cli.git")
        );
        assert_eq!(parsed.options.tag.as_deref(), Some("v3.6.2"));
    }

    #[test]
    fn parse_ssh_git_url_with_reference() {
        let parsed = parse_add_package_spec(
            "git@github.com:r-lib/cli.git@main",
            DEFAULT_GIT_SHORTHAND_BASE_URL,
        )
        .unwrap();
        assert_eq!(parsed.name, "cli");
        assert_eq!(
            parsed.options.git.as_deref(),
            Some("git@github.com:r-lib/cli.git")
        );
        assert_eq!(parsed.options.reference.as_deref(), Some("main"));
    }

    #[test]
    fn parse_owner_repo_uses_custom_git_base_url() {
        let parsed =
            parse_add_package_spec("corp/team-pkg", "https://git.example.com/scm").unwrap();
        assert_eq!(parsed.name, "team-pkg");
        assert_eq!(
            parsed.options.git.as_deref(),
            Some("https://git.example.com/scm/corp/team-pkg")
        );
    }

    // Comprehensive tests - realistic combinations

    #[test]
    fn add_git_comprehensive() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["mypkg".to_string()],
            AddOptions {
                git: Some("https://github.com/user/repo".to_string()),
                tag: Some("v1.0.0".to_string()),
                directory: Some("subdir".to_string()),
                install_suggestions: true,
                dependencies_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_repository_comprehensive() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["dplyr".to_string()],
            AddOptions {
                repository: Some("ppm".to_string()),
                force_source: true,
                install_suggestions: true,
                dependencies_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_local_comprehensive() {
        let mut doc = read_and_verify_config(BASELINE_ADD_CONFIG).unwrap();
        add_packages(
            &mut doc,
            vec!["mypkg".to_string()],
            AddOptions {
                path: Some("../local/package".to_string()),
                install_suggestions: true,
                dependencies_only: true,
                ..Default::default()
            },
        )
        .unwrap();
        insta::assert_snapshot!(doc.to_string());
    }
}
