use assert_cmd::cargo;
use std::fs;
use tempfile::TempDir;

fn create_project() -> (TempDir, TempDir, std::path::PathBuf) {
    let temp = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
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
    (temp, cache, config)
}

#[test]
fn run_clears_r_libs() {
    let (temp, cache, config) = create_project();
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.env("RV_CACHE_DIR", cache.path());
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
    let (temp, cache, config) = create_project();
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.env("RV_CACHE_DIR", cache.path());
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
fn run_forwards_exit_code() {
    let (temp, cache, config) = create_project();
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.env("RV_CACHE_DIR", cache.path());
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd.args(["run", "-e", "quit(status=42)"]);

    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn run_sanitizes_r_env() {
    let (temp, cache, config) = create_project();
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.env("RV_CACHE_DIR", cache.path());
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd.env("R_HOME", "/bogus/r/home");
    cmd.env("R_INCLUDE_DIR", "/bogus/include");
    cmd.args([
        "run",
        "-e",
        r#"cat(Sys.getenv("R_HOME"), Sys.getenv("R_INCLUDE_DIR"), sep="\n")"#,
    ]);

    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("/bogus/r/home"),
        "R_HOME should be overridden"
    );
    assert!(
        !stdout.contains("/bogus/include"),
        "R_INCLUDE_DIR should have been removed"
    );
}
