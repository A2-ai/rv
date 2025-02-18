use std::path::Path;

use std::fs;
use toml_edit::{Array, DocumentMut, Formatted, Item, Value};

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

    let table = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .ok_or(ConfigEditError{
            path: config_file.into(),
            source: ConfigEditErrorKind::NoField("project".to_string())
        })?;

    let dependencies = table.get_mut("dependencies")
        .and_then(|item| item.as_array_mut())
        .ok_or(ConfigEditError{
            path: config_file.into(),
            source: ConfigEditErrorKind::NoField("dependencies".to_string())
        })?;
    let tmp = dependencies.decor_mut();
    tmp.set_suffix(String::new());

    for d in deps {
        if !is_elem_of_array(&dependencies, &d) {
            let mut value = Value::String(Formatted::new(d));
            let decor = value.decor_mut();
            decor.set_prefix("    ");
            dependencies.push(value);
        }
    }
    println!("{}", doc.to_string());
    Ok(())
}

fn is_elem_of_array(arr: &Array, elem: &str) -> bool {
    for a in arr {
        match a {
            Value::String(s) => {
                return s.value() == elem
            },
            Value::InlineTable(t) => {
                if !t.contains_key("repository") {
                    continue;
                }
                if let Some(Value::String(s)) = t.get("name")
                {
                    return s.value() == elem
                }
                
            }
            _ => continue
        }
    }
    false
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
            vec!["dplyr".to_string()],
            "example_projects/rspm-cran/rproject.toml",
        )
        .unwrap();
    }
}
