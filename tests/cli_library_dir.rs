use assert_cmd::cargo;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn create_test_project() -> (TempDir, std::path::PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rproject.toml");

    let config_content = r#"[project]
name = "test"
r_version = "4.5"
repositories = [
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = []
"#;

    fs::write(&config_path, config_content).unwrap();
    (temp_dir, config_path)
}

fn create_test_project_with_library(lib: &str) -> (TempDir, std::path::PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rproject.toml");

    let config_content = format!(
        r#"library = "{lib}"

[project]
name = "test"
r_version = "4.5"
repositories = [
    {{alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}}
]
dependencies = []
"#
    );

    fs::write(&config_path, config_content).unwrap();
    (temp_dir, config_path)
}

fn rv_cmd(cache: &Path, config: &Path) -> assert_cmd::Command {
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.env("RV_CACHE_DIR", cache);
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd
}

#[test]
fn test_default_library_path() {
    let cache = TempDir::new().unwrap();
    let (temp_dir, config_path) = create_test_project();

    let mut cmd = rv_cmd(cache.path(), &config_path);
    cmd.arg("library");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let library_path = stdout.trim();

    // Default library path should be under rv/library/ in the project dir
    assert!(
        library_path.starts_with(&temp_dir.path().to_string_lossy().to_string()),
        "library path should be under project dir, got: {library_path}"
    );
    assert!(
        library_path.contains("rv/library"),
        "default library path should contain rv/library, got: {library_path}"
    );
}

#[test]
fn test_env_var_absolute_path() {
    let cache = TempDir::new().unwrap();
    let (_temp_dir, config_path) = create_test_project();
    let custom_lib = TempDir::new().unwrap();

    let mut cmd = rv_cmd(cache.path(), &config_path);
    cmd.env("RV_LIBRARY_DIR", custom_lib.path());
    cmd.arg("library");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let library_path = stdout.trim();

    assert_eq!(
        library_path,
        custom_lib.path().to_string_lossy().to_string(),
        "RV_LIBRARY_DIR absolute path should be used directly"
    );
}

#[test]
fn test_env_var_relative_path() {
    let cache = TempDir::new().unwrap();
    let (temp_dir, config_path) = create_test_project();

    let mut cmd = rv_cmd(cache.path(), &config_path);
    cmd.env("RV_LIBRARY_DIR", "my-libs");
    cmd.arg("library");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let library_path = stdout.trim();

    let expected = temp_dir.path().join("my-libs");
    assert_eq!(
        library_path,
        expected.to_string_lossy().to_string(),
        "RV_LIBRARY_DIR relative path should resolve against project dir"
    );
}

#[test]
fn test_env_var_overrides_config() {
    let cache = TempDir::new().unwrap();
    let (_temp_dir, config_path) = create_test_project_with_library("libs");
    let custom_lib = TempDir::new().unwrap();

    let mut cmd = rv_cmd(cache.path(), &config_path);
    cmd.env("RV_LIBRARY_DIR", custom_lib.path());
    cmd.arg("library");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let library_path = stdout.trim();

    // Env var should win over config file
    assert_eq!(
        library_path,
        custom_lib.path().to_string_lossy().to_string(),
        "RV_LIBRARY_DIR should override library config in rproject.toml"
    );
}

#[test]
fn test_config_library_without_env_var() {
    let cache = TempDir::new().unwrap();
    let (temp_dir, config_path) = create_test_project_with_library("libs");

    let mut cmd = rv_cmd(cache.path(), &config_path);
    // Explicitly remove RV_LIBRARY_DIR to ensure it's not set
    cmd.env_remove("RV_LIBRARY_DIR");
    cmd.arg("library");

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let library_path = stdout.trim();

    let expected = temp_dir.path().join("libs");
    assert_eq!(
        library_path,
        expected.to_string_lossy().to_string(),
        "config library should be used when RV_LIBRARY_DIR is not set"
    );
}
