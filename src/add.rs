use std::path::Path;

use std::fs;
use toml_edit::{Array, DocumentMut, Formatted, Value};

use crate::{config::ConfigLoadError, Config};


/// Add packages to the config file at path as Simple ConfigDependencies
pub fn add_packages(path: impl AsRef<Path>, packages: Vec<String>) -> Result<String, AddError> {
    let path = path.as_ref();
    let content = fs::read_to_string(path)
        .map_err(|e| AddError {
            path: path.into(),
            source: Box::new(AddErrorKind::Io(e)),
        })?;
    // Verify the config file is valid before parsing as DocumentMut
    content.parse::<Config>().map_err(|e| AddError {
        path: path.into(),
        source: Box::new(AddErrorKind::ConfigLoad(e)),
    })?;

    let mut config_doc = content.parse::<DocumentMut>().unwrap();
    add_pkgs_to_config(&mut config_doc, packages)?;

    Ok(config_doc.to_string())
}

fn add_pkgs_to_config(config_doc: &mut DocumentMut, packages: Vec<String>) -> Result<(), AddError> {
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
    for d in packages {
        if !config_dep_names.contains(&d) {
            config_deps.push(Value::String(Formatted::new(d)));
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
    use std::fs;

    use super::add_pkgs_to_config;

    #[test]
    fn add_remove() {
        let config_file = fs::read_to_string("src/tests/valid_config/all_fields.toml").unwrap();
        let mut doc = config_file.parse::<toml_edit::DocumentMut>().unwrap();
        add_pkgs_to_config(&mut doc, vec!["pkg1".to_string(), "pkg2".to_string()]).unwrap();
        insta::assert_snapshot!("add_remove", doc.to_string());
    }
}
