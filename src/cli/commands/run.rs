use std::str::FromStr;
use std::sync::LazyLock;

use anyhow::{Result, bail};
use regex::Regex;
use toml::{Table, Value};

use crate::{Config, Repository};

static SCRIPT_CONFIG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^# /// rv$\s(?<content>(^#(| .*)$\s)+)^# ///$").unwrap());

pub fn extract_script_config(
    script: &str,
    r_version: &str,
    repositories: &[Repository],
) -> Result<Option<(Config, String)>> {
    if let Some(cap) = SCRIPT_CONFIG_RE.captures(script) {
        let content = match cap.get(1).map(|m| m.as_str()) {
            Some(content) => content,
            None => bail!("Failed to extract config from script"),
        };
        let toml_content = content
            .lines()
            .map(|x| x.trim_start_matches("#"))
            .collect::<Vec<&str>>();

        let mut value: Value = toml::from_str(&toml_content.join("\n"))?;
        if let Some(table) = value.as_table_mut() {
            // Nest it correctly if needed
            if !table.contains_key("project") {
                let mut project_table = Table::new();
                project_table.insert("project".to_string(), Value::Table(table.clone()));
                *table = project_table;
            }

            let project = match table.get_mut("project").and_then(|v| v.as_table_mut()) {
                Some(project) => project,
                None => {
                    bail!("Invalid config section for the script");
                }
            };
            if !project.contains_key("name") {
                project.insert("name".to_string(), Value::String("script".to_string()));
            }
            if !project.contains_key("r_version") {
                project.insert(
                    "r_version".to_string(),
                    Value::String(r_version.to_string()),
                );
            }
            if !project.contains_key("repositories") {
                let repos: Vec<_> = repositories
                    .iter()
                    .map(|r| {
                        let mut t = Table::new();
                        t.insert("alias".to_string(), Value::String(r.alias.clone()));
                        t.insert("url".to_string(), Value::String(r.url().to_string()));
                        Value::Table(t)
                    })
                    .collect();
                project.insert("repositories".to_string(), Value::Array(repos));
            }
        }

        let config_text = toml::to_string(&value)?;
        let config = Config::from_str(&config_text)?;
        Ok(Some((config, config_text)))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_extract_minimal_config() {
        let content = r#"
# /// rv
# dependencies = ["dplyr", "cli"]
# ///

print("hello")
"#;

        let config = extract_script_config(content, "4.5", &[])
            .unwrap()
            .unwrap()
            .0;
        assert_eq!(config.r_version().original, "4.5");
    }

    #[test]
    fn can_extract_minimal_config_with_r_version() {
        let content = r#"
# /// rv
# r_version = "4.6"
# dependencies = ["dplyr", "cli"]
# ///

print("hello")
"#;

        let config = extract_script_config(content, "4.5", &[])
            .unwrap()
            .unwrap()
            .0;
        assert_eq!(config.r_version().original, "4.6");
    }

    #[test]
    fn can_extract_minimal_config_with_r_version_and_repos() {
        let content = r#"
# /// rv
# r_version = "4.6"
# dependencies = ["dplyr", "cli"]
# repositories = [
#    {alias = "posit", url = "https://packagemanager.posit.co/cran/2025-05-12/"}
# ]
# ///

print("hello")
"#;

        let config = extract_script_config(content, "4.5", &[])
            .unwrap()
            .unwrap()
            .0;
        assert_eq!(config.r_version().original, "4.6");
        assert_eq!(config.repositories()[0].alias, "posit");
    }

    #[test]
    fn can_extract_regular_config_with_r_version_and_repos() {
        let content = r#"
# /// rv
# [project]
# r_version = "4.6"
# dependencies = ["dplyr", "cli"]
# repositories = [
#    {alias = "posit", url = "https://packagemanager.posit.co/cran/2025-05-12/"}
# ]
# ///

print("hello")
"#;

        let config = extract_script_config(content, "4.5", &[])
            .unwrap()
            .unwrap()
            .0;
        assert_eq!(config.r_version().original, "4.6");
        assert_eq!(config.repositories()[0].alias, "posit");
    }
}
