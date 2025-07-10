use std::path::Path;
use anyhow::{anyhow, Result};
use fs_err::write;
use serde::Serialize;
use toml_edit::{Array, DocumentMut, Formatted, InlineTable, Value};
use url::Url;

use crate::read_and_verify_config;

#[derive(Debug, Serialize)]
struct ConfigureRepositoryResponse {
    operation: String,
    alias: Option<String>,
    url: Option<String>,
    success: bool,
    message: String,
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to configure repository in config at `{path}`")]
#[non_exhaustive]
pub struct ConfigureError {
    path: Box<Path>,
    #[source]
    source: Box<ConfigureErrorKind>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigureErrorKind {
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
    #[error("Duplicate alias: {0}")]
    DuplicateAlias(String),
    #[error("Alias not found: {0}")]
    AliasNotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml_edit::TomlError),
    #[error("Config load error: {0}")]
    ConfigLoad(#[from] crate::config::ConfigLoadError),
}

pub fn configure_repository(
    config_file: &Path,
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
    is_json_output: bool,
) -> Result<()> {
    let mut doc = read_and_verify_config(config_file)?;

    // Handle different operations and track what we did
    let operation;
    let response_alias;
    let response_url;
    let message;
    
    if clear {
        clear_repositories(&mut doc)?;
        operation = "clear".to_string();
        response_alias = None;
        response_url = None;
        message = "All repositories cleared".to_string();
    } else if let Some(remove_alias) = remove {
        remove_repository(&mut doc, &remove_alias)?;
        operation = "remove".to_string();
        response_alias = Some(remove_alias);
        response_url = None;
        message = "Repository removed successfully".to_string();
    } else {
        // For other operations, we need alias and url
        let alias = alias.ok_or_else(|| anyhow!("--alias is required for this operation"))?;
        let url = url.ok_or_else(|| anyhow!("--url is required for this operation"))?;
        
        // Validate URL only when needed
        let parsed_url = Url::parse(&url)?;
        
        if let Some(replace_alias) = replace {
            replace_repository(&mut doc, &replace_alias, &alias, &parsed_url, force_source)?;
            operation = "replace".to_string();
            message = "Repository replaced successfully".to_string();
        } else {
            add_repository(
                &mut doc,
                &alias,
                &parsed_url,
                force_source,
                before,
                after,
                first,
                last,
            )?;
            operation = "add".to_string();
            message = "Repository configured successfully".to_string();
        }
        
        response_alias = Some(alias);
        response_url = Some(parsed_url.to_string());
    }

    // Write the updated configuration
    write(config_file, doc.to_string())?;

    // Output result
    if is_json_output {
        let response = ConfigureRepositoryResponse {
            operation,
            alias: response_alias,
            url: response_url,
            success: true,
            message,
        };
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        // Print detailed text output similar to JSON structure
        match operation.as_str() {
            "add" => {
                println!("Repository '{}' added successfully with URL: {}", 
                         response_alias.as_ref().unwrap(), 
                         response_url.as_ref().unwrap());
            }
            "replace" => {
                println!("Repository replaced successfully - new alias: '{}', URL: {}", 
                         response_alias.as_ref().unwrap(), 
                         response_url.as_ref().unwrap());
            }
            "remove" => {
                println!("Repository '{}' removed successfully", 
                         response_alias.as_ref().unwrap());
            }
            "clear" => {
                println!("All repositories cleared successfully");
            }
            _ => println!("{}", message),
        }
    }

    Ok(())
}

fn get_mut_repositories_array(doc: &mut DocumentMut) -> Result<&mut Array> {
    let project_table = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .ok_or_else(|| anyhow!("Missing [project] table"))?;
    
    let repos = project_table
        .entry("repositories")
        .or_insert_with(|| Array::new().into())
        .as_array_mut()
        .ok_or_else(|| anyhow!("repositories field is not an array"))?;

    Ok(repos)
}

fn clear_repositories(doc: &mut DocumentMut) -> Result<()> {
    let repos = get_mut_repositories_array(doc)?;
    repos.clear();
    Ok(())
}

fn remove_repository(doc: &mut DocumentMut, alias: &str) -> Result<()> {
    let repos = get_mut_repositories_array(doc)?;
    
    let index = find_repository_index(repos, alias)
        .ok_or_else(|| anyhow!("Repository with alias '{}' not found", alias))?;
    
    repos.remove(index);
    Ok(())
}

fn replace_repository(
    doc: &mut DocumentMut,
    replace_alias: &str,
    new_alias: &str,
    url: &Url,
    force_source: bool,
) -> Result<()> {
    let repos = get_mut_repositories_array(doc)?;
    
    let index = find_repository_index(repos, replace_alias)
        .ok_or_else(|| anyhow!("Repository with alias '{}' not found", replace_alias))?;
    
    // Check for duplicate alias (unless we're replacing with the same alias)
    if new_alias != replace_alias && find_repository_index(repos, new_alias).is_some() {
        return Err(anyhow!("Repository with alias '{}' already exists", new_alias));
    }
    
    let new_repo = create_repository_value(new_alias, url, force_source);
    repos.replace(index, new_repo);
    
    Ok(())
}

fn add_repository(
    doc: &mut DocumentMut,
    alias: &str,
    url: &Url,
    force_source: bool,
    before: Option<String>,
    after: Option<String>,
    first: bool,
    last: bool,
) -> Result<()> {
    let repos = get_mut_repositories_array(doc)?;
    
    // Check for duplicate alias
    if find_repository_index(repos, alias).is_some() {
        return Err(anyhow!("Repository with alias '{}' already exists", alias));
    }
    
    let new_repo = create_repository_value(alias, url, force_source);
    
    let insert_index = if first {
        0
    } else if last {
        repos.len()
    } else if let Some(before_alias) = before {
        find_repository_index(repos, &before_alias)
            .ok_or_else(|| anyhow!("Repository with alias '{}' not found", before_alias))?
    } else if let Some(after_alias) = after {
        let after_index = find_repository_index(repos, &after_alias)
            .ok_or_else(|| anyhow!("Repository with alias '{}' not found", after_alias))?;
        after_index + 1
    } else {
        // Default to last
        repos.len()
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
    for (i, item) in repos.iter_mut().enumerate() {
        if i == 0 {
            item.decor_mut().set_prefix("\n    ");
        } else {
            item.decor_mut().set_prefix("\n    ");
        }
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
    
    #[test]
    fn test_add_first() {
        let (_temp_dir, config_path) = create_test_config();
        
        configure_repository(
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
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_first", result);
    }
    
    #[test]
    fn test_add_after() {
        let (_temp_dir, config_path) = create_test_config();
        
        configure_repository(
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
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_after", result);
    }
    
    #[test]
    fn test_add_before() {
        let (_temp_dir, config_path) = create_test_config();
        
        configure_repository(
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
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_add_before", result);
    }
    
    #[test]
    fn test_replace() {
        let (_temp_dir, config_path) = create_test_config();
        
        configure_repository(
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
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_replace", result);
    }
    
    #[test]
    fn test_remove() {
        let (_temp_dir, config_path) = create_test_config();
        
        configure_repository(
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
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_remove", result);
    }
    
    #[test]
    fn test_clear() {
        let (_temp_dir, config_path) = create_test_config();
        
        configure_repository(
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
            false,
        ).unwrap();
        
        let result = fs::read_to_string(&config_path).unwrap();
        insta::assert_snapshot!("configure_clear", result);
    }
    
    #[test]
    fn test_duplicate_alias_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let result = configure_repository(
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
            false,
        );
        
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }
    
    #[test] 
    fn test_invalid_url_error() {
        let (_temp_dir, config_path) = create_test_config();
        
        let result = configure_repository(
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
            false,
        );
        
        assert!(result.is_err());
    }
}

