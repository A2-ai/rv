use std::path::Path;

use std::fs;
use fs_err::write;
use toml_edit::{Array, DocumentMut, Formatted, Value};

pub fn add_dependencies(config_file: impl AsRef<Path>, deps: Vec<String>) -> Result<(), ConfigEditError> {
    config_edit(config_file, deps, add_fxn)
}

pub fn remove_dependencies(config_file: impl AsRef<Path>, deps: Vec<String>) -> Result<(), ConfigEditError> {
    config_edit(config_file, deps, remove_fxn)
}

// add and remove are almost completely the same except for the mutation of the dependency array. 
// Thus one function calling different mutation functions instead of two functions calling many shared functions
fn config_edit<F>(config_file: impl AsRef<Path>, deps: Vec<String>, edit_fxn: F) -> Result<(), ConfigEditError>
where
    F: Fn(&mut Array, Vec<String>),
{
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

    edit_fxn(project_dependencies, deps);

    project_dependencies.set_trailing("\n");
    project_dependencies.set_trailing_comma(true);

    write(config_file, doc.to_string())
        .map_err(|e| ConfigEditError {
            path: config_file.into(),
            source: ConfigEditErrorKind::Io(e),
        })?;

    Ok(())
}

fn add_fxn(config_deps: &mut Array, add_deps: Vec<String>) {
    let config_dep_names = config_deps.iter().filter_map(|v| get_dependency_name(v)).collect::<Vec<_>>();
    for d in add_deps {
        if !config_dep_names.contains(&d) {
            config_deps.push(Value::String(Formatted::new(d)));
            // Couldn't format value before pushing, so adding formatting after its added
            if let Some(last) = config_deps.iter_mut().last() {
                last.decor_mut().set_prefix("\n    ");
            }        
        }
    }
}

fn remove_fxn(config_deps: &mut Array, add_deps: Vec<String>) {
    config_deps.retain(|v| {
        if let Some(name) = get_dependency_name(v) {
            !add_deps.contains(&name)
        } else {
            true
        }
    });
}

fn get_dependency_name(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.value().to_string()),
            Value::InlineTable(t) => {
                t.get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            },
            _ => None
    }
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
    use std::path::Path;

    use fs_err::{read_to_string, write};
    use tempfile::tempdir;

    #[test]
    fn tester() {
        let content = read_to_string("src/tests/valid_config/all_fields.toml").unwrap();
        let mut out = String::new();
        let tmp_dir = tempdir().unwrap();
        let config_file = tmp_dir.path().join("rproject.toml");
        write(&config_file, content).unwrap();
        super::add_dependencies(&config_file, vec!["test_pkg".to_string(), "dplyr".to_string()]).unwrap();
        out.push_str("===ADDED==========");
        out.push_str(&read_to_string(&config_file).unwrap());
        super::remove_dependencies(&config_file, vec!["test_pkg".to_string(), "dplyr".to_string(), "renv".to_string()]).unwrap();
        out.push_str("===REMOVED==========");
        out.push_str(&read_to_string(&config_file).unwrap());
        insta::assert_snapshot!("add_remove", out);
    }
}
