use assert_cmd::cargo;
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

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "add",
        "cran",
        "--url",
        "https://cran.r-project.org",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!("cli_add_stdout", String::from_utf8_lossy(&output.stdout));

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_add_config", result);
}

#[test]
fn test_configure_repository_add_with_positioning() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "add",
        "cran",
        "--url",
        "https://cran.r-project.org",
        "--first",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!(
        "cli_add_first_stdout",
        String::from_utf8_lossy(&output.stdout)
    );

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_add_first_config", result);
}

#[test]
fn test_configure_repository_replace() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "replace",
        "posit",
        "--url",
        "https://packagemanager.posit.co/cran/latest",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!(
        "cli_replace_stdout",
        String::from_utf8_lossy(&output.stdout)
    );

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_replace_config", result);
}

#[test]
fn test_configure_repository_replace_with_new_alias() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "replace",
        "posit",
        "--alias",
        "posit-new",
        "--url",
        "https://packagemanager.posit.co/cran/latest",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!(
        "cli_replace_new_alias_stdout",
        String::from_utf8_lossy(&output.stdout)
    );

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_replace_new_alias_config", result);
}

#[test]
fn test_configure_repository_update_alias() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "update",
        "posit",
        "--alias",
        "posit-updated",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!(
        "cli_update_alias_stdout",
        String::from_utf8_lossy(&output.stdout)
    );

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_update_alias_config", result);
}

#[test]
fn test_configure_repository_update_url() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "update",
        "posit",
        "--url",
        "https://packagemanager.posit.co/cran/latest",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!(
        "cli_update_url_stdout",
        String::from_utf8_lossy(&output.stdout)
    );

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_update_url_config", result);
}

#[test]
fn test_configure_repository_update_force_source() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "update",
        "posit",
        "--force-source",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!(
        "cli_update_force_source_stdout",
        String::from_utf8_lossy(&output.stdout)
    );

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_update_force_source_config", result);
}

#[test]
fn test_configure_repository_update_by_url() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "update",
        "--match-url",
        "https://packagemanager.posit.co/cran/2024-12-16/",
        "--alias",
        "matched-by-url",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!(
        "cli_update_by_url_stdout",
        String::from_utf8_lossy(&output.stdout)
    );

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_update_by_url_config", result);
}

#[test]
fn test_configure_repository_remove() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "remove",
        "posit",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!("cli_remove_stdout", String::from_utf8_lossy(&output.stdout));

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_remove_config", result);
}

#[test]
fn test_configure_repository_clear() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "clear",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the stdout
    insta::assert_snapshot!("cli_clear_stdout", String::from_utf8_lossy(&output.stdout));

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_clear_config", result);
}

#[test]
fn test_configure_repository_json_output() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "--json",
        "configure",
        "repository",
        "add",
        "cran",
        "--url",
        "https://cran.r-project.org",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    // Snapshot the JSON stdout
    insta::assert_snapshot!(
        "cli_json_output_stdout",
        String::from_utf8_lossy(&output.stdout)
    );

    // Snapshot the resulting config
    let result = fs::read_to_string(&config_path).unwrap();
    insta::assert_snapshot!("cli_json_output_config", result);
}

#[test]
fn test_configure_repository_error_missing_alias() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "update",
        "--url",
        "https://example.com",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(!output.status.success());

    // Snapshot the stderr
    insta::assert_snapshot!(
        "cli_error_missing_alias_stderr",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_configure_repository_error_nonexistent_alias() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "update",
        "nonexistent",
        "--url",
        "https://example.com",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(!output.status.success());

    // Verify error contains expected message without path-specific part
    let stderr_str = String::from_utf8_lossy(&output.stderr);
    assert!(stderr_str.contains("Alias not found: nonexistent"));

    // Snapshot just the essential error part (removing dynamic paths)
    insta::assert_snapshot!(
        "cli_error_nonexistent_alias_stderr",
        "Failed to configure repository\n\nCaused by:\n    Alias not found: nonexistent"
    );
}

#[test]
fn test_configure_repository_conflict_flags() {
    let (_temp_dir, config_path) = create_test_config();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "configure",
        "repository",
        "add",
        "cran",
        "--url",
        "https://cran.r-project.org",
        "--first",
        "--last",
        "--config-file",
        config_path.to_str().unwrap(),
    ]);

    let output = cmd.output().unwrap();
    assert!(!output.status.success());

    // Normalize binary name for cross-platform compatibility (rv vs rv.exe)
    let stderr_str = String::from_utf8_lossy(&output.stderr).replace("rv.exe", "rv");
    insta::assert_snapshot!("cli_conflict_flags_stderr", stderr_str);
}
