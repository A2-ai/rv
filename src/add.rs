use std::path::Path;

use fs_err::write;
use std::fs;
use toml_edit::{Array, DocumentMut, Formatted, Value};

pub fn add_packages(packages: Vec<String>, config_file: impl AsRef<Path>) -> Result<(), AddError> {
    let config_file = config_file.as_ref();
    let config_content = fs::read_to_string(&config_file).map_err(|e| AddError {
        path: config_file.into(),
        source: AddErrorKind::Io(e),
    })?;

    let mut doc = config_content
        .parse::<DocumentMut>()
        .map_err(|e| AddError {
            path: config_file.into(),
            source: AddErrorKind::Parse(e),
        })?;

    // get the dependencies array
    let config_deps = get_mut_array(&mut doc);

    // collect the names of all of the dependencies
    let config_dep_names = config_deps
        .iter()
        .filter_map(|v| get_dependency_name(v))
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

    // write back out the file
    write(config_file, doc.to_string()).map_err(|e| AddError {
        path: config_file.into(),
        source: AddErrorKind::Io(e),
    })?;

    Ok(())
}

fn get_mut_array(doc: &mut DocumentMut) -> &mut Array {
    // config arrays are behind the "project" table
    let table = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .unwrap();
    // get the array or insert it if it does not exist
    let deps = table
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
pub struct AddError {
    path: Box<Path>,
    source: AddErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum AddErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] toml_edit::TomlError),
}

#[cfg(test)]
mod tests {
    use fs_err::{read_to_string, write};
    use tempfile::tempdir;

    use crate::add_packages;

    #[test]
    fn add_remove() {
        let content = read_to_string("src/tests/valid_config/all_fields.toml").unwrap();
        let tmp_dir = tempdir().unwrap();
        let config_file = tmp_dir.path().join("rproject.toml");
        write(&config_file, content).unwrap();
        add_packages(vec!["pkg1".to_string(), "pkg2".to_string()], &config_file).unwrap();
        insta::assert_snapshot!("add_remove", read_to_string(&config_file).unwrap());
    }
}
