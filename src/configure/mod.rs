mod dependency;
mod repository;

pub use repository::{
    ConfigureRepositoryResponse, RepositoryAction, RepositoryMatcher, RepositoryOperation,
    RepositoryPositioning, RepositoryUpdates, execute_repository_action,
};

pub use dependency::{ConfigureDependencyResponse, DependencyAction, execute_dependency_action};

use std::path::Path;
use toml_edit::{Array, DocumentMut, Formatted, Value};

use crate::{Config, config::ConfigLoadError};

#[derive(Debug, Clone, Copy)]
enum ConfigureType {
    Repository,
    Dependency,
}

impl ConfigureType {
    fn as_str(&self) -> &str {
        match self {
            Self::Repository => "repositories",
            Self::Dependency => "dependencies",
        }
    }
}

fn read_config_as_document(config_file: &Path) -> Result<DocumentMut, ConfigLoadError> {
    // Verify config can be loaded and is valid
    let _ = Config::from_file(config_file)?;

    // Read and parse as DocumentMut for editing
    let config_content = std::fs::read_to_string(config_file).map_err(|e| ConfigLoadError {
        path: config_file.into(),
        source: crate::config::ConfigLoadErrorKind::Io(e),
    })?;

    config_content
        .parse::<DocumentMut>()
        .map_err(|e| ConfigLoadError {
            path: config_file.into(),
            source: crate::config::ConfigLoadErrorKind::InvalidConfig(e.to_string()),
        })
}

fn get_mut_array(
    doc: &mut DocumentMut,
    config_type: ConfigureType,
) -> Result<&mut Array, ConfigureErrorKind> {
    let project_table = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .ok_or(ConfigureErrorKind::MissingProjectTable)?;

    let array = project_table
        .entry(config_type.as_str())
        .or_insert_with(|| Array::new().into())
        .as_array_mut()
        .ok_or(ConfigureErrorKind::InvalidField)?;

    Ok(array)
}

fn format_array(array: &mut Array) {
    // Remove any existing formatting
    for item in array.iter_mut() {
        item.decor_mut().clear();
    }

    // Add proper formatting
    for item in array.iter_mut() {
        item.decor_mut().set_prefix("\n    ");
    }

    // Set trailing formatting
    array.set_trailing("\n");
    array.set_trailing_comma(true);
}

fn clear_array(
    doc: &mut DocumentMut,
    config_type: ConfigureType,
) -> Result<(), ConfigureErrorKind> {
    let array = get_mut_array(doc, config_type)?;
    array.clear();
    Ok(())
}

fn to_value_string(s: impl AsRef<str>) -> Value {
    Value::String(Formatted::new(s.as_ref().to_string()))
}

fn to_value_bool(b: bool) -> Value {
    Value::Boolean(Formatted::new(b))
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to configure repository in config at `{path}`")]
#[non_exhaustive]
pub struct ConfigureError {
    path: Box<Path>,
    #[source]
    source: Box<ConfigureErrorKind>,
}

impl ConfigureError {
    pub fn with_path(mut self, path: impl Into<Box<Path>>) -> Self {
        self.path = path.into();
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigureErrorKind {
    #[error("Invalid URL: {0}")]
    InvalidUrl(url::ParseError),
    #[error("Duplicate alias: {0}")]
    DuplicateAlias(String),
    #[error("Duplicate package name: {0}")]
    DuplicatePackage(String),
    #[error("Alias not found: {0}")]
    AliasNotFound(String),
    #[error("Package(s) not found: {0}")]
    PackagesNotFound(String),
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("Config load error: {0}")]
    ConfigLoad(ConfigLoadError),
    #[error("Missing [project] table")]
    MissingProjectTable,
    #[error("specified field is not an array")]
    InvalidField,
}
