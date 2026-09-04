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

#[test]
fn run_self_contained_works() {
    let cache = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scripts/self_contained.R");

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(cwd.path());
    cmd.env("RV_CACHE_DIR", cache.path());
    cmd.args(["run", script.to_str().unwrap()]);

    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("SELF_CONTAINED_OK"),);

    assert!(cache.path().join("scripts").exists());
}

#[test]
fn run_self_contained_ignores_ambient_project() {
    let cache = TempDir::new().unwrap();
    let temp = TempDir::new().unwrap();
    // An ambient rv project pointing at a much more recent repository than the script does.
    fs::write(
        temp.path().join("rproject.toml"),
        r#"[project]
name = "ambient"
r_version = "4.5"
repositories = [
    { alias = "posit", url = "https://packagemanager.posit.co/cran/2025-05-12/" },
]
dependencies = ["glue"]
"#,
    )
    .unwrap();
    // ... and activated, so R would load an `.Rprofile` that points `.libPaths()` at the project
    // library. That must not shadow the library rv set up for the script.
    let shadow = temp.path().join("shadow-library");
    fs::create_dir(&shadow).unwrap();
    fs::write(
        temp.path().join(".Rprofile"),
        format!(
            ".libPaths({:?}, include.site = FALSE)\n",
            shadow.to_str().unwrap()
        ),
    )
    .unwrap();
    fs::write(
        temp.path().join(".Renviron"),
        format!("R_LIBS_USER={}\n", shadow.to_str().unwrap()),
    )
    .unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/scripts/self_contained_in_project.R");

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.current_dir(temp.path());
    cmd.env("RV_CACHE_DIR", cache.path());
    cmd.args(["run", script.to_str().unwrap()]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // glue 1.6.2 is what the 2023-06-01 snapshot has; the ambient project would give 1.8.x
    assert!(stdout.contains("VERSION: 1.6.2"));
    // and it must load from the script library in the cache, not the project library
    let scripts_dir = cache.path().join("scripts");
    assert!(scripts_dir.exists());
    let library_line = stdout
        .lines()
        .find(|l| l.starts_with("LIBRARY:"))
        .expect("no LIBRARY line in output");
    assert!(library_line.contains(scripts_dir.to_str().unwrap()));
    assert!(!temp.path().join("rv").exists());
}
