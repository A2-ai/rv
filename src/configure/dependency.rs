use std::{ops::Deref, path::Path};

use fs_err::write;
use toml_edit::{Array, DocumentMut, InlineTable, Value};

use crate::{
    configure::{
        format_array, get_mut_array, read_config_as_document, repository::find_repository_index, to_value_bool, to_value_string, ConfigureError, ConfigureErrorKind, ConfigureType
    }, ConfigDependency
};

pub enum DependencyAction {
    Add { config_dep: ConfigDependency },
}

enum DependencyOperation {
    Add,
}

pub struct ConfigureDependencyResponse {
    operation: DependencyOperation,
    name: String,
}

pub fn execute_dependency_action<'a>(
    config_file: impl AsRef<Path>,
    action: DependencyAction,
) -> Result<ConfigureDependencyResponse, ConfigureError> {
    let config_file = config_file.as_ref();
    let mut doc = read_config_as_document(config_file).map_err(|e| ConfigureError {
        path: config_file.into(),
        source: Box::new(ConfigureErrorKind::ConfigLoad(e)),
    })?;

    let res = match action {
        DependencyAction::Add { config_dep } => {
            add_dependency(&mut doc, &config_dep).map_err(|e| ConfigureError {
                path: config_file.into(),
                source: Box::new(e),
            })?;
            ConfigureDependencyResponse {
                operation: DependencyOperation::Add,
                name: config_dep.name().to_string(),
            }
        }
    };

    write(config_file, doc.to_string()).map_err(|e| ConfigureError {
        path: config_file.into(),
        source: Box::new(ConfigureErrorKind::Io(e)),
    })?;

    Ok(res)
}

fn add_dependency(
    doc: &mut DocumentMut,
    config_dep: &ConfigDependency,
) -> Result<(), ConfigureErrorKind> {
    if let ConfigDependency::Detailed { repository: Some(alias), .. } = config_dep {
        let repos = get_mut_array(doc, ConfigureType::Repository)?;
        if find_repository_index(repos, alias).is_none() {
            return Err(ConfigureErrorKind::AliasNotFound(alias.to_string()))
        }
    }

    let deps = get_mut_array(doc, ConfigureType::Dependency)?;
    if find_dependency_index(deps, config_dep.name()).is_some() {
        return Err(ConfigureErrorKind::DuplicatePackage(
            config_dep.name().to_string(),
        ));
    }

    let new_dep = create_dependency_value(config_dep);
    deps.push(new_dep);

    format_array(deps);

    Ok(())
}

fn find_dependency_index(deps: &Array, name: &str) -> Option<usize> {
    deps.iter().position(|dep| match dep {
        Value::InlineTable(tbl) => tbl
            .get("name")
            .and_then(|v| v.as_str())
            .map(|n| n == name)
            .unwrap_or(false),
        Value::String(s) => &s.to_string() == name,
        _ => false,
    })
}

fn create_dependency_value(config_dep: &ConfigDependency) -> Value {
    fn insert_option(tbl: &mut InlineTable, install_suggestions: bool, dependencies_only: bool) {
        if install_suggestions {
            tbl.insert("install_suggestion", to_value_bool(true));
        }
        if dependencies_only {
            tbl.insert("dependnecies_only", to_value_bool(true));
        }
    }

    match config_dep {
        ConfigDependency::Simple(name) => to_value_string(name),
        ConfigDependency::Detailed {
            name,
            repository,
            install_suggestions,
            force_source,
            dependencies_only,
        } => {
            let mut tbl = InlineTable::new();
            tbl.insert("name", to_value_string(name));
            if let Some(repo) = repository {
                tbl.insert("repository", to_value_string(repo));
            }
            if let Some(fs) = force_source {
                tbl.insert("force_source", to_value_bool(*fs));
            }
            insert_option(&mut tbl, *install_suggestions, *dependencies_only);

            Value::InlineTable(tbl)
        }
        ConfigDependency::Local {
            path,
            name,
            install_suggestions,
            dependencies_only,
        } => {
            let mut tbl = InlineTable::new();
            tbl.insert("name", to_value_string(name));
            tbl.insert("path", to_value_string(path.to_string_lossy()));
            insert_option(&mut tbl, *install_suggestions, *dependencies_only);

            Value::InlineTable(tbl)
        }
        ConfigDependency::Url {
            url,
            name,
            install_suggestions,
            dependencies_only,
        } => {
            let mut tbl = InlineTable::new();
            tbl.insert("name", to_value_string(name));
            tbl.insert("url", to_value_string(url.deref().as_str()));
            insert_option(&mut tbl, *install_suggestions, *dependencies_only);

            Value::InlineTable(tbl)
        }
        ConfigDependency::Git {
            git,
            commit,
            tag,
            branch,
            directory,
            name,
            install_suggestions,
            dependencies_only,
        } => {
            let mut tbl = InlineTable::new();
            tbl.insert("name", to_value_string(name));
            tbl.insert("git", to_value_string(git.url()));
            match (tag, branch, commit) {
                (Some(tag), None, None) => tbl.insert("tag", to_value_string(tag)),
                (None, Some(branch), None) => tbl.insert("branch", to_value_string(branch)),
                (None, None, Some(commit)) => tbl.insert("commit", to_value_string(commit)),
                _ => unreachable!("Only one tag, branch, commit allowed to be specified"),
            };
            if let Some(dir) = directory {
                tbl.insert("directory", to_value_string(dir));
            }
            insert_option(&mut tbl, *install_suggestions, *dependencies_only);

            Value::InlineTable(tbl)
        }
    }
}
