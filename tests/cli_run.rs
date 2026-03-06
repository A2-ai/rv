use assert_cmd::cargo;
use std::fs;
use tempfile::TempDir;

fn create_project() -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let config = temp.path().join("rproject.toml");
    fs::write(
        &config,
        r#"[project]
name = "test-run"
r_version = "4.5"
repositories = []
dependencies = []
"#,
    )
    .unwrap();
    (temp, config)
}

#[test]
fn run_clears_r_libs() {
    let (temp, config) = create_project();
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd.env("R_LIBS", "/should/not/appear");
    cmd.args(["run", "-e", r#"cat(Sys.getenv("R_LIBS"))"#]);

    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "", "R_LIBS should be empty, got: {stdout}");
}

#[test]
fn run_sets_library_path() {
    let (temp, config) = create_project();
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd.args(["run", "-e", r#"cat(Sys.getenv("R_LIBS_USER"))"#]);

    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("rv/library") || stdout.contains("rv\\library"),
        "expected library path in stdout, got: {stdout}"
    );
}

#[test]
fn run_activates_project() {
    let (temp, config) = create_project();
    // No activation files exist yet
    assert!(!temp.path().join(".Rprofile").exists());

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd.args(["run", "-e", "cat('ok')"]);
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // After rv run, activation files should exist
    assert!(temp.path().join(".Rprofile").exists());
    assert!(temp.path().join("rv/scripts/activate.R").exists());
}

#[test]
fn run_forwards_exit_code() {
    let (temp, config) = create_project();
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd.args(["run", "-e", "quit(status=42)"]);

    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(42));
}
