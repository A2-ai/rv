use assert_cmd::cargo;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Create a test project with a single, already-known dependency (R6) and a posit repo.
fn create_test_project() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rproject.toml");

    let config_content = r#"[project]
name = "test"
r_version = "4.5"
repositories = [
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = [
    "R6",
]
"#;

    fs::write(&config_path, config_content).unwrap();
    (temp_dir, config_path)
}

#[test]
#[ignore]
fn test_add_failure_is_atomic() {
    let cache = TempDir::new().unwrap();
    let (_temp_dir, config_path) = create_test_project();
    let original_config = fs::read_to_string(&config_path).unwrap();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.env("RV_CACHE_DIR", cache.path()).args([
        "--config-file",
        config_path.to_str().unwrap(),
        "add",
        "jsonlite",
        "rv-no-such-package-xyz",
    ]);

    let output = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(stderr.contains("The rproject.toml hasn't been modified."),);
    assert_eq!(fs::read_to_string(&config_path).unwrap(), original_config,);
}
