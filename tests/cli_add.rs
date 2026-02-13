use assert_cmd::cargo;
use std::fs;
use tempfile::TempDir;

#[test]
fn test_add_does_not_persist_config_when_sync_fails() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rproject.toml");

    // Use a deliberately unavailable R version so `add` fails after editing in-memory config.
    let config_content = r#"[project]
name = "test"
r_version = "99.99"
repositories = [
    { alias = "cran", url = "https://cran.r-project.org" }
]
dependencies = [
    "R6",
]
"#;
    fs::write(&config_path, config_content).unwrap();

    let before = fs::read_to_string(&config_path).unwrap();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "add",
        "dplyr",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);
    let output = cmd.output().unwrap();

    assert!(
        !output.status.success(),
        "expected add to fail\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let after = fs::read_to_string(&config_path).unwrap();
    assert_eq!(after, before);
}
