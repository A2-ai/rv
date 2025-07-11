use std::path::Path;
use fs_err::write;
use serde::Serialize;
use toml_edit::{Array, DocumentMut, Formatted, InlineTable, Value};
use url::Url;

use crate::{Config, config::ConfigLoadError};

fn read_config_as_document(config_file: &Path) -> Result<DocumentMut, ConfigLoadError> {
    // Verify config can be loaded and is valid
    let _ = Config::from_file(config_file)?;
    
    // Read and parse as DocumentMut for editing
    let config_content = std::fs::read_to_string(config_file)
        .map_err(|e| ConfigLoadError {
            path: config_file.into(),
            source: crate::config::ConfigLoadErrorKind::Io(e),
        })?;
    
    config_content.parse::<DocumentMut>()
        .map_err(|e| ConfigLoadError {
            path: config_file.into(),
            source: crate::config::ConfigLoadErrorKind::InvalidConfig(e.to_string()),
        })
}



#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryOperation {
    Add,
    Replace,
    Remove,
    Clear,
}

#[derive(Debug)]
pub enum RepositoryPositioning {
    First,
    Last,
    Before(String),
    After(String),
}

#[derive(Debug)]
pub enum RepositoryAction {
    Add {
        alias: String,
        url: Url,
        positioning: RepositoryPositioning,
        force_source: bool,
    },
    Replace {
        old_alias: String,
        new_alias: String,
        url: Url,
        force_source: bool,
    },
    Remove {
        alias: String,
    },
    Clear,
}

#[derive(Debug, Serialize)]
struct ConfigureRepositoryResponse {
    operation: RepositoryOperation,
    alias: Option<String>,
    url: Option<String>,
    success: bool,
    message: String,
}

#[derive(Debug)]
pub struct CliArgs {
    pub alias: Option<String>,
    pub url: Option<String>,
    pub force_source: bool,
    pub before: Option<String>,
    pub after: Option<String>,
    pub first: bool,
    pub last: bool,
    pub replace: Option<String>,
    pub remove: Option<String>,
    pub clear: bool,
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
    #[error("Alias not found: {0}")]
    AliasNotFound(String),
    #[error("--alias is required for this operation")]
    MissingAlias,
    #[error("--url is required for this operation")]
    MissingUrl,
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("Config load error: {0}")]
    ConfigLoad(ConfigLoadError),
    #[error("JSON serialization error: {0}")]
    SerdeJson(serde_json::Error),
    #[error("Missing [project] table")]
    MissingProjectTable,
    #[error("repositories field is not an array")]
    InvalidRepositoriesField,
}

pub fn execute_repository_action(
    config_file: &Path,
    action: RepositoryAction,
    is_json_output: bool,
) -> Result<(), ConfigureError> {
    let mut doc = read_config_as_document(config_file)
        .map_err(|e| ConfigureError {
            path: config_file.into(),
            source: Box::new(ConfigureErrorKind::ConfigLoad(e)),
        })?;

    // Handle different operations and track what we did
    let (operation, response_alias, response_url, message) = match action {
        RepositoryAction::Clear => {
            clear_repositories(&mut doc)
                .map_err(|e| ConfigureError {
                    path: config_file.into(),
                    source: Box::new(e),
                })?;
            (RepositoryOperation::Clear, None, None, "All repositories cleared".to_string())
        }
        
        RepositoryAction::Remove { alias } => {
            remove_repository(&mut doc, &alias)
                .map_err(|e| ConfigureError {
                    path: config_file.into(),
                    source: Box::new(e),
                })?;
            (RepositoryOperation::Remove, Some(alias), None, "Repository removed successfully".to_string())
        }
        
        RepositoryAction::Replace { old_alias, new_alias, url, force_source } => {
            replace_repository(&mut doc, &old_alias, &new_alias, &url, force_source)
                .map_err(|e| ConfigureError {
                    path: config_file.into(),
                    source: Box::new(e),
                })?;
            (RepositoryOperation::Replace, Some(new_alias), Some(url.to_string()), "Repository replaced successfully".to_string())
        }
        
        RepositoryAction::Add { alias, url, positioning, force_source } => {
            add_repository(&mut doc, &alias, &url, positioning, force_source)
                .map_err(|e| ConfigureError {
                    path: config_file.into(),
                    source: Box::new(e),
                })?;
            (RepositoryOperation::Add, Some(alias), Some(url.to_string()), "Repository configured successfully".to_string())
        }
    };

    // Write the updated configuration
    write(config_file, doc.to_string())
        .map_err(|e| ConfigureError {
            path: config_file.into(),
            source: Box::new(ConfigureErrorKind::Io(e)),
        })?;

    // Output result
    if is_json_output {
        let response = ConfigureRepositoryResponse {
            operation,
            alias: response_alias,
            url: response_url,
            success: true,
            message,
        };
        println!("{}", serde_json::to_string_pretty(&response)
            .map_err(|e| ConfigureError {
                path: config_file.into(), 
                source: Box::new(ConfigureErrorKind::SerdeJson(e)),
            })?);
    } else {
        // Print detailed text output similar to JSON structure
        match operation {
            RepositoryOperation::Add => {
                println!("Repository '{}' added successfully with URL: {}", 
                         response_alias.as_ref().unwrap(), 
                         response_url.as_ref().unwrap());
            }
            RepositoryOperation::Replace => {
                println!("Repository replaced successfully - new alias: '{}', URL: {}", 
                         response_alias.as_ref().unwrap(), 
                         response_url.as_ref().unwrap());
            }
            RepositoryOperation::Remove => {
                println!("Repository '{}' removed successfully", 
                         response_alias.as_ref().unwrap());
            }
            RepositoryOperation::Clear => {
                println!("All repositories cleared successfully");
            }
        }
    }

    Ok(())
}


pub fn parse_repository_action(args: CliArgs) -> Result<RepositoryAction, ConfigureError> {
    if args.clear {
        return Ok(RepositoryAction::Clear);
    }
    
    if let Some(remove_alias) = args.remove {
        return Ok(RepositoryAction::Remove { alias: remove_alias });
    }
    
    // For add/replace operations, we need alias and url
    let alias = args.alias.ok_or_else(|| ConfigureError {
        path: std::path::Path::new("").into(), // Will be updated by caller
        source: Box::new(ConfigureErrorKind::MissingAlias),
    })?;
    let url = args.url.ok_or_else(|| ConfigureError {
        path: std::path::Path::new("").into(), // Will be updated by caller
        source: Box::new(ConfigureErrorKind::MissingUrl),
    })?;
    
    // Validate URL
    let parsed_url = Url::parse(&url)
        .map_err(|e| ConfigureError {
            path: std::path::Path::new("").into(), // Will be updated by caller
            source: Box::new(ConfigureErrorKind::InvalidUrl(e)),
        })?;
    
    if let Some(old_alias) = args.replace {
        return Ok(RepositoryAction::Replace {
            old_alias,
            new_alias: alias,
            url: parsed_url,
            force_source: args.force_source,
        });
    }
    
    // Determine positioning
    let positioning = if args.first {
        RepositoryPositioning::First
    } else if args.last {
        RepositoryPositioning::Last
    } else if let Some(before_alias) = args.before {
        RepositoryPositioning::Before(before_alias)
    } else if let Some(after_alias) = args.after {
        RepositoryPositioning::After(after_alias)
    } else {
        RepositoryPositioning::Last // Default
    };
    
    Ok(RepositoryAction::Add {
        alias,
        url: parsed_url,
        positioning,
        force_source: args.force_source,
    })
}


fn get_mut_repositories_array(doc: &mut DocumentMut) -> Result<&mut Array, ConfigureErrorKind> {
    let project_table = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .ok_or(ConfigureErrorKind::MissingProjectTable)?;
    
    let repos = project_table
        .entry("repositories")
        .or_insert_with(|| Array::new().into())
        .as_array_mut()
        .ok_or(ConfigureErrorKind::InvalidRepositoriesField)?;

    Ok(repos)
}

fn clear_repositories(doc: &mut DocumentMut) -> Result<(), ConfigureErrorKind> {
    let repos = get_mut_repositories_array(doc)?;
    repos.clear();
    Ok(())
}

fn remove_repository(doc: &mut DocumentMut, alias: &str) -> Result<(), ConfigureErrorKind> {
    let repos = get_mut_repositories_array(doc)?;
    
    let index = find_repository_index(repos, alias)
        .ok_or_else(|| ConfigureErrorKind::AliasNotFound(alias.to_string()))?;
    
    repos.remove(index);
    Ok(())
}

fn replace_repository(
    doc: &mut DocumentMut,
    replace_alias: &str,
    new_alias: &str,
    url: &Url,
    force_source: bool,
) -> Result<(), ConfigureErrorKind> {
    let repos = get_mut_repositories_array(doc)?;
    
    let index = find_repository_index(repos, replace_alias)
        .ok_or_else(|| ConfigureErrorKind::AliasNotFound(replace_alias.to_string()))?;
    
    // Check for duplicate alias (unless we're replacing with the same alias)
    if new_alias != replace_alias && find_repository_index(repos, new_alias).is_some() {
        return Err(ConfigureErrorKind::DuplicateAlias(new_alias.to_string()));
    }
    
    let new_repo = create_repository_value(new_alias, url, force_source);
    repos.replace(index, new_repo);
    
    Ok(())
}

fn add_repository(
    doc: &mut DocumentMut,
    alias: &str,
    url: &Url,
    positioning: RepositoryPositioning,
    force_source: bool,
) -> Result<(), ConfigureErrorKind> {
    let repos = get_mut_repositories_array(doc)?;
    
    // Check for duplicate alias
    if find_repository_index(repos, alias).is_some() {
        return Err(ConfigureErrorKind::DuplicateAlias(alias.to_string()));
    }
    
    let new_repo = create_repository_value(alias, url, force_source);
    
    let insert_index = match positioning {
        RepositoryPositioning::First => 0,
        RepositoryPositioning::Last => repos.len(),
        RepositoryPositioning::Before(before_alias) => {
            find_repository_index(repos, &before_alias)
                .ok_or(ConfigureErrorKind::AliasNotFound(before_alias))?
        }
        RepositoryPositioning::After(after_alias) => {
            let after_index = find_repository_index(repos, &after_alias)
                .ok_or(ConfigureErrorKind::AliasNotFound(after_alias))?;
            after_index + 1
        }
    };
    
    repos.insert(insert_index, new_repo);
    
    // Format the array properly
    format_repositories_array(repos);
    
    Ok(())
}

fn find_repository_index(repos: &Array, alias: &str) -> Option<usize> {
    repos.iter().position(|repo| {
        repo.as_inline_table()
            .and_then(|table| table.get("alias"))
            .and_then(|v| v.as_str())
            .map(|a| a == alias)
            .unwrap_or(false)
    })
}

fn create_repository_value(alias: &str, url: &Url, force_source: bool) -> Value {
    let mut table = InlineTable::new();
    table.insert("alias", Value::String(Formatted::new(alias.to_string())));
    table.insert("url", Value::String(Formatted::new(url.to_string())));
    
    if force_source {
        table.insert("force_source", Value::Boolean(Formatted::new(true)));
    }
    
    Value::InlineTable(table)
}

fn format_repositories_array(repos: &mut Array) {
    // Remove any existing formatting
    for item in repos.iter_mut() {
        item.decor_mut().clear();
    }
    
    // Add proper formatting
    for item in repos.iter_mut() {
        item.decor_mut().set_prefix("\n    ");
    }
    
    // Set trailing formatting
    repos.set_trailing("\n");
    repos.set_trailing_comma(true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    
    fn execute_test_action(
        config_path: &std::path::Path,
        alias: Option<String>,
        url: Option<String>,
        force_source: bool,
        before: Option<String>,
        after: Option<String>,
        first: bool,
        last: bool,
        replace: Option<String>,
        remove: Option<String>,
        clear: bool,
    ) -> Result<(), ConfigureError> {
        let cli_args = CliArgs {
            alias, url, force_source, before, after, first, last, replace, remove, clear
        };
        
        let action = parse_repository_action(cli_args)?;
        execute_repository_action(config_path, action, false)
    }

    fn create_test_config() -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("rproject.toml");
        
        let config_content = r#"[project]
name = "test"
r_version = "4.4"
repositories = [
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = [
    "dplyr",
]
"#;
        
        fs::write(&config_path, config_content).unwrap();
        (temp_dir, config_path)
    }
    
    #[test]
    fn test_add_first() {
        let (_temp_dir, config_path) = create_test_config();
        
        execute_test_action(
            &config_path,
            Some("ppm".to_string()),
            Some("https://packagemanager.posit.co/cran/latest".to_string()),
            false,
            None,
            None,
            true,
            false,
            None,
            None,
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_first", result);
    }
    
    #[test]
    fn test_add_after() {
        let (_temp_dir, config_path) = create_test_config();
        
        execute_test_action(
            &config_path,
            Some("ppm-old".to_string()),
            Some("https://packagemanager.posit.co/cran/2024-11-16".to_string()),
            false,
            None,
            Some("posit".to_string()),
            false,
            false,
            None,
            None,
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_after", result);
    }
    
    #[test]
    fn test_add_before() {
        let (_temp_dir, config_path) = create_test_config();
        
        execute_test_action(
            &config_path,
            Some("ppm".to_string()),
            Some("https://packagemanager.posit.co/cran/latest".to_string()),
            false,
            Some("posit".to_string()),
            None,
            false,
            false,
            None,
            None,
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_before", result);
    }
    
    #[test]
    fn test_replace() {
        let (_temp_dir, config_path) = create_test_config();
        
        execute_test_action(
            &config_path,
            Some("ppm".to_string()),
            Some("https://packagemanager.posit.co/cran/latest".to_string()),
            false,
            None,
            None,
            false,
            false,
            Some("posit".to_string()),
            None,
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_replace", result);
    }
    
    #[test]
    fn test_remove() {
        let (_temp_dir, config_path) = create_test_config();
        
        execute_test_action(
            &config_path,
            None,
            None,
            false,
            None,
            None,
            false,
            false,
            None,
            Some("posit".to_string()),
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_remove", result);
    }
    
    #[test]
    fn test_clear() {
        let (_temp_dir, config_path) = create_test_config();
        
        execute_test_action(
            &config_path,
            None,
            None,
            false,
            None,
            None,
            false,
            false,
            None,
            None,
            true,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_clear", result);
    }
    
    #[test]
    fn test_duplicate_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let result = execute_test_action(
            &config_path,
            Some("posit".to_string()),
            Some("https://packagemanager.posit.co/cran/latest".to_string()),
            false,
            None,
            None,
            false,
            false,
            None,
            None,
            false,
        );
        
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(format!("{:?}", error.source).contains("DuplicateAlias"));
    }
    
    #[test] 
    fn test_invalid_url_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let result = execute_test_action(
            &config_path,
            Some("invalid".to_string()),
            Some("not-a-url".to_string()),
            false,
            None,
            None,
            false,
            false,
            None,
            None,
            false,
        );
        
        assert!(result.is_err());
    }
}

