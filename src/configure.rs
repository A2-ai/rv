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



#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryOperation {
    Add,
    Replace,
    Update,
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
    Update {
        matcher: RepositoryMatcher,
        updates: RepositoryUpdates,
    },
    Remove {
        alias: String,
    },
    Clear,
}

#[derive(Debug)]
pub enum RepositoryMatcher {
    ByAlias(String),
    ByUrl(Url),
}

#[derive(Debug)]
pub struct RepositoryUpdates {
    pub alias: Option<String>,
    pub url: Option<Url>,
    pub force_source: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ConfigureRepositoryResponse {
    pub operation: RepositoryOperation,
    pub alias: Option<String>,
    pub url: Option<String>,
    pub success: bool,
    pub message: String,
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
    #[error("IO error: {0}")]
    Io(std::io::Error),
    #[error("Config load error: {0}")]
    ConfigLoad(ConfigLoadError),
    #[error("Missing [project] table")]
    MissingProjectTable,
    #[error("repositories field is not an array")]
    InvalidRepositoriesField,
}

pub fn execute_repository_action(
    config_file: &Path,
    action: RepositoryAction,
) -> Result<ConfigureRepositoryResponse, ConfigureError> {
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
        
        RepositoryAction::Update { matcher, updates } => {
            let (old_alias, response_alias, response_url) = update_repository(&mut doc, &matcher, &updates)
                .map_err(|e| ConfigureError {
                    path: config_file.into(),
                    source: Box::new(e),
                })?;
            (RepositoryOperation::Update, response_alias, response_url, format!("Repository '{}' updated successfully", old_alias))
        }
    };

    // Write the updated configuration
    write(config_file, doc.to_string())
        .map_err(|e| ConfigureError {
            path: config_file.into(),
            source: Box::new(ConfigureErrorKind::Io(e)),
        })?;

    // Return response data for CLI to handle output
    Ok(ConfigureRepositoryResponse {
        operation,
        alias: response_alias,
        url: response_url,
        success: true,
        message,
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

fn update_repository(
    doc: &mut DocumentMut,
    matcher: &RepositoryMatcher,
    updates: &RepositoryUpdates,
) -> Result<(String, Option<String>, Option<String>), ConfigureErrorKind> {
    let repos = get_mut_repositories_array(doc)?;
    
    // Find the repository to update
    let index = match matcher {
        RepositoryMatcher::ByAlias(alias) => {
            find_repository_index(repos, alias)
                .ok_or_else(|| ConfigureErrorKind::AliasNotFound(alias.clone()))?
        }
        RepositoryMatcher::ByUrl(url) => {
            find_repository_index_by_url(repos, url)
                .ok_or_else(|| ConfigureErrorKind::AliasNotFound(format!("URL: {}", url)))?
        }
    };
    
    // Get the current repository
    let current_repo = repos.get(index).unwrap().as_inline_table().unwrap();
    let old_alias = current_repo.get("alias").unwrap().as_str().unwrap().to_string();
    let current_url = current_repo.get("url").unwrap().as_str().unwrap();
    let current_force_source = current_repo.get("force_source")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    
    // Apply updates
    let new_alias = updates.alias.as_ref().unwrap_or(&old_alias).clone();
    let new_url = updates.url.as_ref()
        .map(|u| u.to_string())
        .unwrap_or_else(|| current_url.to_string());
    let new_force_source = updates.force_source.unwrap_or(current_force_source);
    
    // Check for duplicate alias (unless we're keeping the same alias)
    if new_alias != old_alias && find_repository_index(repos, &new_alias).is_some() {
        return Err(ConfigureErrorKind::DuplicateAlias(new_alias));
    }
    
    // Create the updated repository
    let parsed_url = Url::parse(&new_url)
        .map_err(|e| ConfigureErrorKind::InvalidUrl(e))?;
    let new_repo = create_repository_value(&new_alias, &parsed_url, new_force_source);
    repos.replace(index, new_repo);
    
    let response_alias = Some(new_alias);
    let response_url = Some(parsed_url.to_string());
    
    Ok((old_alias, response_alias, response_url))
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

fn find_repository_index_by_url(repos: &Array, url: &Url) -> Option<usize> {
    repos.iter().position(|repo| {
        repo.as_inline_table()
            .and_then(|table| table.get("url"))
            .and_then(|v| v.as_str())
            .map(|u| u == url.as_str())
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
    
    fn create_test_config_with_force_source() -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("rproject.toml");
        
        let config_content = r#"[project]
name = "test"
r_version = "4.4"
repositories = [
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/", force_source = true}
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
        
        let action = RepositoryAction::Add {
            alias: "ppm".to_string(),
            url: Url::parse("https://packagemanager.posit.co/cran/latest").unwrap(),
            positioning: RepositoryPositioning::First,
            force_source: false,
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_first", result);
    }
    
    #[test]
    fn test_add_after() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Add {
            alias: "ppm-old".to_string(),
            url: Url::parse("https://packagemanager.posit.co/cran/2024-11-16").unwrap(),
            positioning: RepositoryPositioning::After("posit".to_string()),
            force_source: false,
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_after", result);
    }
    
    #[test]
    fn test_add_before() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Add {
            alias: "ppm".to_string(),
            url: Url::parse("https://packagemanager.posit.co/cran/latest").unwrap(),
            positioning: RepositoryPositioning::Before("posit".to_string()),
            force_source: false,
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_before", result);
    }
    
    #[test]
    fn test_replace() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Replace {
            old_alias: "posit".to_string(),
            new_alias: "ppm".to_string(),
            url: Url::parse("https://packagemanager.posit.co/cran/latest").unwrap(),
            force_source: false,
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_replace", result);
    }
    
    #[test]
    fn test_remove() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Remove {
            alias: "posit".to_string(),
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_remove", result);
    }
    
    #[test]
    fn test_clear() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Clear;
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_clear", result);
    }
    
    #[test]
    fn test_duplicate_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Add {
            alias: "posit".to_string(), // Same as existing alias
            url: Url::parse("https://packagemanager.posit.co/cran/latest").unwrap(),
            positioning: RepositoryPositioning::Last,
            force_source: false,
        };
        
        let result = execute_repository_action(&config_path, action);
        
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(format!("{:?}", error.source).contains("DuplicateAlias"));
    }
    

    #[test]
    fn test_add_last_default() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Add {
            alias: "cran".to_string(),
            url: Url::parse("https://cran.r-project.org").unwrap(),
            positioning: RepositoryPositioning::Last,
            force_source: false,
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_last", result);
    }

    #[test]
    fn test_add_with_force_source() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Add {
            alias: "bioc".to_string(),
            url: Url::parse("https://bioconductor.org/packages/3.18/bioc").unwrap(),
            positioning: RepositoryPositioning::Last,
            force_source: true,
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_force_source", result);
    }

    #[test]
    fn test_replace_with_force_source() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Replace {
            old_alias: "posit".to_string(),
            new_alias: "bioc".to_string(),
            url: Url::parse("https://bioconductor.org/packages/3.18/bioc").unwrap(),
            force_source: true,
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_replace_force_source", result);
    }

    #[test]
    fn test_replace_same_alias() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Replace {
            old_alias: "posit".to_string(),
            new_alias: "posit".to_string(), // Same alias should work
            url: Url::parse("https://packagemanager.posit.co/cran/2024-12-01").unwrap(),
            force_source: false,
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_replace_same_alias", result);
    }

    #[test]
    fn test_before_nonexistent_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Add {
            alias: "new-repo".to_string(),
            url: Url::parse("https://example.com").unwrap(),
            positioning: RepositoryPositioning::Before("nonexistent".to_string()),
            force_source: false,
        };
        
        let result = execute_repository_action(&config_path, action);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(format!("{:?}", error.source).contains("AliasNotFound"));
    }

    #[test]
    fn test_after_nonexistent_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Add {
            alias: "new-repo".to_string(),
            url: Url::parse("https://example.com").unwrap(),
            positioning: RepositoryPositioning::After("nonexistent".to_string()),
            force_source: false,
        };
        
        let result = execute_repository_action(&config_path, action);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(format!("{:?}", error.source).contains("AliasNotFound"));
    }

    #[test]
    fn test_replace_nonexistent_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Replace {
            old_alias: "nonexistent".to_string(),
            new_alias: "new".to_string(),
            url: Url::parse("https://example.com").unwrap(),
            force_source: false,
        };
        
        let result = execute_repository_action(&config_path, action);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(format!("{:?}", error.source).contains("AliasNotFound"));
    }

    #[test]
    fn test_remove_nonexistent_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Remove {
            alias: "nonexistent".to_string(),
        };
        
        let result = execute_repository_action(&config_path, action);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(format!("{:?}", error.source).contains("AliasNotFound"));
    }


    #[test]
    fn test_update_alias_by_alias() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("posit".to_string()),
            updates: RepositoryUpdates {
                alias: Some("posit-updated".to_string()),
                url: None,
                force_source: None,
            },
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_update_alias_by_alias", result);
    }

    #[test]
    fn test_update_url_by_alias() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("posit".to_string()),
            updates: RepositoryUpdates {
                alias: None,
                url: Some(Url::parse("https://packagemanager.posit.co/cran/latest").unwrap()),
                force_source: None,
            },
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_update_url_by_alias", result);
    }

    #[test] 
    fn test_update_force_source_by_alias() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("posit".to_string()),
            updates: RepositoryUpdates {
                alias: None,
                url: None,
                force_source: Some(true),
            },
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_update_force_source_by_alias", result);
    }

    #[test]
    fn test_update_remove_force_source_by_alias() {
        let (_temp_dir, config_path) = create_test_config_with_force_source();
        
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("posit".to_string()),
            updates: RepositoryUpdates {
                alias: None,
                url: None,
                force_source: Some(false),
            },
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_update_remove_force_source_by_alias", result);
    }

    #[test]
    fn test_update_multiple_fields_by_alias() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("posit".to_string()),
            updates: RepositoryUpdates {
                alias: Some("posit-new".to_string()),
                url: Some(Url::parse("https://packagemanager.posit.co/cran/latest").unwrap()),
                force_source: Some(true),
            },
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_update_multiple_fields_by_alias", result);
    }

    #[test]
    fn test_update_by_url() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByUrl(Url::parse("https://packagemanager.posit.co/cran/2024-12-16/").unwrap()),
            updates: RepositoryUpdates {
                alias: Some("posit-matched-by-url".to_string()),
                url: None,
                force_source: Some(true),
            },
        };
        
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_update_by_url", result);
    }

    #[test]
    fn test_update_nonexistent_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("nonexistent".to_string()),
            updates: RepositoryUpdates {
                alias: Some("new-alias".to_string()),
                url: None,
                force_source: None,
            },
        };
        
        let result = execute_repository_action(&config_path, action);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(format!("{:?}", error.source).contains("AliasNotFound"));
    }

    #[test]
    fn test_update_duplicate_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        // Try to update posit to have the same alias as an existing repository
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("posit".to_string()),
            updates: RepositoryUpdates {
                alias: Some("posit".to_string()), // This should be fine (same alias)
                url: None,
                force_source: None,
            },
        };
        
        // This should work (updating to the same alias)
        execute_repository_action(&config_path, action).unwrap();
        
        // Now try with a different existing alias - this should fail
        let _action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("posit".to_string()),
            updates: RepositoryUpdates {
                alias: Some("posit".to_string()), // Wait, we need another repo first
                url: None,
                force_source: None,
            },
        };
        
        // First, let's add another repository so we can test duplicate error
        let add_action = RepositoryAction::Add {
            alias: "cran".to_string(),
            url: Url::parse("https://cran.r-project.org").unwrap(),
            positioning: RepositoryPositioning::Last,
            force_source: false,
        };
        execute_repository_action(&config_path, add_action).unwrap();
        
        // Now try to update posit to have alias "cran" - should fail
        let action = RepositoryAction::Update {
            matcher: RepositoryMatcher::ByAlias("posit".to_string()),
            updates: RepositoryUpdates {
                alias: Some("cran".to_string()),
                url: None,
                force_source: None,
            },
        };
        
        let result = execute_repository_action(&config_path, action);
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(format!("{:?}", error.source).contains("DuplicateAlias"));
    }

    #[test]
    fn test_clear_empty_repositories() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("rproject.toml");
        
        // Create config with empty repositories array
        let config_content = r#"[project]
name = "test"
r_version = "4.4"
repositories = []
dependencies = [
    "dplyr",
]
"#;
        fs::write(&config_path, config_content).unwrap();
        
        let action = RepositoryAction::Clear;
        execute_repository_action(&config_path, action).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_clear_empty", result);
    }
}

