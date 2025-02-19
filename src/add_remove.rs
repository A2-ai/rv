use std::path::Path;

use std::fs;
use toml_edit::{Array, DocumentMut, Formatted, Value};

fn add(deps: Vec<String>, config_file: impl AsRef<Path>) -> Result<(), ConfigEditError> {
    let config_file = config_file.as_ref();
    let config_content = fs::read_to_string(&config_file).map_err(|e| ConfigEditError {
        path: config_file.into(),
        source: ConfigEditErrorKind::Io(e),
    })?;

    let mut doc = config_content
        .parse::<DocumentMut>()
        .map_err(|e| ConfigEditError {
            path: config_file.into(),
            source: ConfigEditErrorKind::Parse(e),
        })?;

    let project_table = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .ok_or(ConfigEditError {
            path: config_file.into(),
            source: ConfigEditErrorKind::NoField("project".to_string()),
        })?;

    let project_dependencies = project_table
        .get_mut("dependencies")
        .and_then(|item| item.as_array_mut())
        .ok_or(ConfigEditError {
            path: config_file.into(),
            source: ConfigEditErrorKind::NoField("dependencies".to_string()),
        })?;

    if let Some(last) = project_dependencies.iter_mut().last() {
        last.decor_mut().set_suffix("");
    }

    let project_repo_dep_names = repository_dependencies(project_dependencies);
    for d in deps {
        if !project_repo_dep_names.contains(&d) {
            project_dependencies.push(Value::String(Formatted::new(d)));
            if let Some(last) = project_dependencies.iter_mut().last() {
                last.decor_mut().set_prefix("\n    ");
            }
        }
    }

    Ok(())
}

fn repository_dependencies(arr: &Array) -> Vec<String> {
    arr
        .iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(s.value().to_string()),
            Value::InlineTable(t) => {
                let name = t.get("name");
                if let Some(Value::String(s)) = name {
                    Some(s.value().to_string())
                } else {
                    None
                }
            },
            _ => None
        })
        .collect::<Vec<_>>()
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to edit config at `{path}`")]
#[non_exhaustive]
pub struct ConfigEditError {
    path: Box<Path>,
    source: ConfigEditErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum ConfigEditErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] toml_edit::TomlError),
    #[error("Could not find required field {0}")]
    NoField(String),
}

mod tests {
    #[test]
    fn tester() {
        super::add(
            vec!["test_pkg".to_string()],
            "example_projects/simple/rproject.toml",
        )
        .unwrap();
    }
}
