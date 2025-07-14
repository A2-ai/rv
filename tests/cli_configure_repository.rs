use assert_cmd::Command;
use predicates::prelude::*;
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
fn test_configure_repository_add() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "add", "cran",
        "--url", "https://cran.r-project.org",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository 'cran' added successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("alias = \"cran\""));
    assert!(result.contains("https://cran.r-project.org"));
}

#[test]
fn test_configure_repository_add_with_positioning() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "add", "cran",
        "--url", "https://cran.r-project.org",
        "--first",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository 'cran' added successfully"));
    
    // Verify the config was updated and cran is first
    let result = fs::read_to_string(&config_path).unwrap();
    let lines: Vec<&str> = result.lines().collect();
    let cran_line = lines.iter().position(|&line| line.contains(r#"alias = "cran""#)).unwrap();
    let posit_line = lines.iter().position(|&line| line.contains(r#"alias = "posit""#)).unwrap();
    assert!(cran_line < posit_line);
}

#[test]
fn test_configure_repository_replace() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "replace", "posit",
        "--url", "https://packagemanager.posit.co/cran/latest",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository replaced successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("https://packagemanager.posit.co/cran/latest"));
    assert!(!result.contains("2024-12-16"));
}

#[test]
fn test_configure_repository_replace_with_new_alias() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "replace", "posit",
        "--alias", "posit-new",
        "--url", "https://packagemanager.posit.co/cran/latest",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository replaced successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("posit-new"));
    assert!(!result.contains("alias = \"posit\""));
}

#[test]
fn test_configure_repository_update_alias() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "update", "posit",
        "--alias", "posit-updated",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository 'posit-updated' updated successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("posit-updated"));
    assert!(!result.contains("alias = \"posit\""));
}

#[test]
fn test_configure_repository_update_url() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "update", "posit",
        "--url", "https://packagemanager.posit.co/cran/latest",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository 'posit' updated successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("https://packagemanager.posit.co/cran/latest"));
    assert!(!result.contains("2024-12-16"));
}

#[test]
fn test_configure_repository_update_force_source() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "update", "posit",
        "--force-source",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository 'posit' updated successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("force_source = true"));
}

#[test]
fn test_configure_repository_update_by_url() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "update",
        "--match-url", "https://packagemanager.posit.co/cran/2024-12-16/",
        "--alias", "matched-by-url",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository 'matched-by-url' updated successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("matched-by-url"));
    assert!(!result.contains("alias = \"posit\""));
}

#[test]
fn test_configure_repository_remove() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "remove", "posit",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Repository 'posit' removed successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    // Check that posit is not in the repositories array specifically
    assert!(!result.contains(r#"alias = "posit""#));
}

#[test]
fn test_configure_repository_clear() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "clear",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("All repositories cleared successfully"));
    
    // Verify the config was updated
    let result = fs::read_to_string(&config_path).unwrap();
    assert!(result.contains("repositories = []"));
}

#[test]
fn test_configure_repository_json_output() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "--json",
        "configure", "repository", "add", "cran",
        "--url", "https://cran.r-project.org",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("\"operation\": \"add\""))
        .stdout(predicate::str::contains("\"alias\": \"cran\""))
        .stdout(predicate::str::contains("\"success\": true"));
}

#[test]
fn test_configure_repository_error_missing_alias() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "update",
        "--url", "https://example.com",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Must specify either target alias or --match-url"));
}

#[test]
fn test_configure_repository_error_nonexistent_alias() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "update", "nonexistent",
        "--url", "https://example.com",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("Alias not found"));
}

#[test]
fn test_configure_repository_conflict_flags() {
    let (_temp_dir, config_path) = create_test_config();
    
    let mut cmd = Command::cargo_bin("rv").unwrap();
    cmd.args(&[
        "configure", "repository", "add", "cran",
        "--url", "https://cran.r-project.org",
        "--first", "--last",
        "--config-file", config_path.to_str().unwrap()
    ]);
    
    cmd.assert()
        .failure();
}