use std::path::Path;

use fs_err::write;
use std::fs;
use toml_edit::{Array, DocumentMut, Formatted, Value};

pub struct Changes {
    add: Vec<String>,
    remove: Vec<String>,
}

impl Changes {
    pub fn new(add: Vec<String>, remove: Vec<String>) -> Self {
        Self {
            add,
            remove
        }
    }

    pub fn edit_config(&self, config_file: impl AsRef<Path>) -> Result<(), ConfigEditError> {
        // Read the configuration file into a DocumentMut for toml editing
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

        // get the dependencies array
        let config_deps = get_mut_array(&mut doc, config_file)?;

        // add/remove the dependencies from the dependencies array
        add_fxn(config_deps, &self.add);
        remove_fxn(config_deps, &self.remove);

        // Set a trailing new line and comma for the last element for proper formatting
        config_deps.set_trailing("\n");
        config_deps.set_trailing_comma(true);

        // write back out the file
        write(config_file, doc.to_string()).map_err(|e| ConfigEditError {
            path: config_file.into(),
            source: ConfigEditErrorKind::Io(e),
        })?;

        Ok(())
    }
}

fn get_mut_array<'a>(doc: &'a mut DocumentMut, config_file: impl AsRef<Path>) -> Result<&'a mut Array, ConfigEditError> {
    let config_file = config_file.as_ref();
    // config arrays are behind the "project" table
    let table = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .ok_or(ConfigEditError {
            path: config_file.into(),
            source: ConfigEditErrorKind::NoField("project".to_string()),
        })?;
    // get the array indexed by the array name
    let deps = table
        .get_mut("dependencies")
        .and_then(|item| item.as_array_mut())
        .ok_or(ConfigEditError {
            path: config_file.into(),
            source: ConfigEditErrorKind::NoField("dependencies".to_string()),
        })?;
    // remove formatting on the last element as we will re-add
    if let Some(last) = deps.iter_mut().last() {
        last.decor_mut().set_suffix("");
    }
    Ok(deps)
}

fn add_fxn(config_deps: &mut Array, add_deps: &Vec<String>) {
    // collect the names of all of the dependencies
    let config_dep_names = config_deps
        .iter()
        .filter_map(|v| get_dependency_name(v))
        .collect::<Vec<_>>();
    // Determine if the dep to add is in the config, if not add it
    for d in add_deps {
        if !config_dep_names.contains(d) {
            config_deps.push(Value::String(Formatted::new(d.to_string())));
            // Couldn't format value before pushing, so adding formatting after its added
            if let Some(last) = config_deps.iter_mut().last() {
                last.decor_mut().set_prefix("\n    ");
            }
        }
    }
}

fn remove_fxn(config_deps: &mut Array, remove_deps: &Vec<String>) {
    // retain config_deps that do not match any of the deps to remove
    config_deps.retain(|v| {
        if let Some(name) = get_dependency_name(v) {
            !remove_deps.contains(&name)
        } else {
            true
        }
    });
}

fn get_dependency_name(value: &Value) -> Option<String> {
    // Our dependencies are currently only formatted as basic Strings and InlineTable
    match value {
        Value::String(s) => Some(s.value().to_string()),
        Value::InlineTable(t) => t
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use fs_err::{read_to_string, write};
    use tempfile::tempdir;

    #[test]
    fn add_remove() {
        let content = read_to_string("src/tests/valid_config/all_fields.toml").unwrap();
        let mut out = String::new();
        let tmp_dir = tempdir().unwrap();
        let config_file = tmp_dir.path().join("rproject.toml");
        write(&config_file, content).unwrap();

        let changes = super::Changes::new(vec!["pkg1".to_string(), "pkg2".to_string()], Vec::new());
        changes.edit_config(&config_file).unwrap();
        out.push_str("===ADD==========");
        out.push_str(&read_to_string(&config_file).unwrap());
        
        let changes = super::Changes::new(Vec::new(), vec!["dplyr".to_string()]);
        changes.edit_config(&config_file).unwrap();
        out.push_str("===REMOVE==========");
        out.push_str(&read_to_string(&config_file).unwrap());

        let changes = super::Changes::new(vec!["dplyr".to_string()], vec!["pkg1".to_string(), "pkg2".to_string()]);
        changes.edit_config(&config_file).unwrap();
        out.push_str("===REVERT==========");
        out.push_str(&read_to_string(&config_file).unwrap());

        insta::assert_snapshot!("add_remove", out);
    }
}
