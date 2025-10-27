use std::path::Path;

use std::fs;
use toml_edit::{Array, DocumentMut, Formatted, InlineTable, Value};

use crate::{Config, config::ConfigLoadError};

#[derive(Debug, Clone, Default)]
pub struct AddOptions {
    pub repository: Option<String>,
    pub force_source: bool,
    pub install_suggestions: bool,
    pub dependencies_only: bool,
    pub git: Option<String>,
    pub commit: Option<String>,
    pub tag: Option<String>,
    pub branch: Option<String>,
    pub directory: Option<String>,
    pub path: Option<String>,
    pub url: Option<String>,
}

pub fn read_and_verify_config(config_file: impl AsRef<Path>) -> Result<DocumentMut, AddError> {
    let config_file = config_file.as_ref();
    let _ = Config::from_file(config_file).map_err(|e| AddError {
        path: config_file.into(),
        source: Box::new(AddErrorKind::ConfigLoad(e)),
    })?;
    let config_content = fs::read_to_string(config_file).unwrap(); // Verified config could be loaded above

    Ok(config_content.parse::<DocumentMut>().unwrap()) // Verify config was valid toml above
}

pub fn add_packages(
    config_doc: &mut DocumentMut,
    packages: Vec<String>,
    options: AddOptions,
) -> Result<(), AddError> {
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

fn create_dependency_value(package_name: &str, options: &AddOptions) -> Result<Value, AddError> {
    // Check if this is a simple string dependency (no options)
    let is_simple = options.repository.is_none()
        && !options.force_source
        && !options.install_suggestions
        && !options.dependencies_only
        && options.git.is_none()
        && options.path.is_none()
        && options.url.is_none();

    if is_simple {
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
        }

        if let Some(ref directory) = options.directory {
            table.insert("directory", Value::from(directory.as_str()));
        }

        if options.install_suggestions {
            table.insert("install_suggestions", Value::from(true));
        }

        if options.dependencies_only {
            table.insert("dependencies_only", Value::from(true));
        }
    } else if let Some(ref path) = options.path {
        // Local path dependency
        table.insert("path", Value::from(path.as_str()));

        if options.install_suggestions {
            table.insert("install_suggestions", Value::from(true));
        }

        if options.dependencies_only {
            table.insert("dependencies_only", Value::from(true));
        }
    } else if let Some(ref url) = options.url {
        // URL dependency
        table.insert("url", Value::from(url.as_str()));

        if options.install_suggestions {
            table.insert("install_suggestions", Value::from(true));
        }

        if options.dependencies_only {
            table.insert("dependencies_only", Value::from(true));
        }
    } else {
        // Detailed/repository dependency
        if let Some(ref repository) = options.repository {
            table.insert("repository", Value::from(repository.as_str()));
        }

        if options.force_source {
            table.insert("force_source", Value::from(true));
        }

        if options.install_suggestions {
            table.insert("install_suggestions", Value::from(true));
        }

        if options.dependencies_only {
            table.insert("dependencies_only", Value::from(true));
        }
    }

    Ok(Value::InlineTable(table))
}

fn get_mut_array(doc: &mut DocumentMut) -> &mut Array {
    // the dependnecies array is behind the project table
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

#[derive(Debug, thiserror::Error)]
#[error("Failed to edit config at `{path}`")]
#[non_exhaustive]
pub struct AddError {
    path: Box<Path>,
    source: Box<AddErrorKind>,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum AddErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] toml_edit::TomlError),
    ConfigLoad(#[from] ConfigLoadError),
}

#[cfg(test)]
mod tests {
    use super::AddOptions;
    use crate::{add_packages, read_and_verify_config};

    const BASELINE_CONFIG: &str = "src/tests/valid_config/baseline_for_add.toml";

    // Simple tests - one feature at a time

    #[test]
    fn add_simple_package() {
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
        add_packages(&mut doc, vec!["dplyr".to_string()], AddOptions::default()).unwrap();
        insta::assert_snapshot!(doc.to_string());
    }

    #[test]
    fn add_with_repository() {
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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

    // Comprehensive tests - realistic combinations

    #[test]
    fn add_git_comprehensive() {
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
        let mut doc = read_and_verify_config(BASELINE_CONFIG).unwrap();
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
