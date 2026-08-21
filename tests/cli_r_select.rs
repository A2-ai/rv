use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo;
use tempfile::TempDir;

/// A project depending on R6, with its own cache dir. Returns (cache, project, config path).
fn setup_project(r_version: &str) -> (TempDir, TempDir, PathBuf) {
    let cache = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let config = project.path().join("rproject.toml");
    fs::write(
        &config,
        format!(
            r#"[project]
name = "test"
r_version = "{r_version}"
repositories = [
    {{alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}}
]
dependencies = ["R6"]
"#
        ),
    )
    .unwrap();
    (cache, project, config)
}

fn rv_sync(cache: &Path, config: &Path, extra: &[&str]) -> assert_cmd::Command {
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.env("RV_CACHE_DIR", cache);
    cmd.args(["-v", "--config-file", config.to_str().unwrap(), "sync"]);
    cmd.args(extra);
    cmd
}

/// The R on PATH (local or CI) as (binary path, `major.minor`).
fn r_on_path() -> (PathBuf, String) {
    let r = rv::r_finder::get_r_from_path().expect("R must be on PATH for this test");
    let [major, minor] = r.version.major_minor();
    (r.bin_path, format!("{major}.{minor}"))
}

/// A `major.minor` guaranteed to differ from `mm` (same major, absurd minor).
fn other_version(mm: &str) -> String {
    let (major, minor) = mm.split_once('.').unwrap();
    format!("{major}.{}", minor.parse::<u32>().unwrap() + 50)
}

fn write_lockfile(path: &Path, r_version: &str) {
    fs::write(
        path,
        format!(
            r#"version = 2
r_version = "{r_version}"

[[packages]]
name = "R6"
version = "2.5.1"
source = {{ repository = "https://packagemanager.posit.co/cran/2024-12-16/" }}
force_source = false
dependencies = []
"#
        ),
    )
    .unwrap();
}

#[test]
fn sync_with_missing_r_bin_path_errors() {
    let (cache, project, config) = setup_project("4.5");

    rv_sync(
        cache.path(),
        &config,
        &[
            "--r-bin",
            project.path().join("no-such-R").to_str().unwrap(),
        ],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("Could not find R version"));
}

#[test]
fn sync_with_r_version_alone_mismatching_config_errors() {
    let (cache, _project, config) = setup_project("4.4");

    rv_sync(cache.path(), &config, &["--r-version", "4.5"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("--r-bin"));
}

#[test]
#[ignore]
fn sync_with_matching_r_bin_uses_and_writes_lockfile() {
    let (r_bin, mm) = r_on_path();
    let (cache, project, config) = setup_project(&mm);

    rv_sync(cache.path(), &config, &["--r-bin", r_bin.to_str().unwrap()])
        .assert()
        .success();

    let lockfile = fs::read_to_string(project.path().join("rv.lock")).unwrap();
    assert!(lockfile.contains(&format!(r#"r_version = "{mm}""#)));
    assert!(lockfile.contains("R6"));
    assert!(project.path().join(format!("rv/library/{mm}")).exists());
}

#[test]
#[ignore]
fn sync_with_r_version_alone_matching_config_syncs() {
    let (_r_bin, mm) = r_on_path();
    let (cache, project, config) = setup_project(&mm);

    rv_sync(cache.path(), &config, &["--r-version", &mm])
        .assert()
        .success();

    let lockfile = fs::read_to_string(project.path().join("rv.lock")).unwrap();
    assert!(lockfile.contains(&format!(r#"r_version = "{mm}""#)));
}

#[test]
#[ignore]
fn sync_with_mismatched_r_bin_alone_errors_and_preserves_lockfile() {
    let (r_bin, mm) = r_on_path();
    let other = other_version(&mm);
    let (cache, project, config) = setup_project(&other);
    let lockfile_path = project.path().join("rv.lock");
    write_lockfile(&lockfile_path, &other);
    let before = fs::read_to_string(&lockfile_path).unwrap();

    rv_sync(cache.path(), &config, &["--r-bin", r_bin.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Pass --r-version"));

    assert_eq!(fs::read_to_string(&lockfile_path).unwrap(), before);
    assert!(!project.path().join(format!("rv/library/{other}")).exists());
}

#[test]
#[ignore]
fn sync_combo_mismatch_succeeds_and_preserves_lockfile() {
    let (r_bin, mm) = r_on_path();
    let other = other_version(&mm);
    let (cache, project, config) = setup_project(&other);
    let lockfile_path = project.path().join("rv.lock");
    write_lockfile(&lockfile_path, &other);
    let before = fs::read_to_string(&lockfile_path).unwrap();

    rv_sync(
        cache.path(),
        &config,
        &["--r-bin", r_bin.to_str().unwrap(), "--r-version", &mm],
    )
    .assert()
    .success()
    .stderr(predicates::str::contains("does not match the config"));

    assert_eq!(fs::read_to_string(&lockfile_path).unwrap(), before);
    // library is built at the binary's version, not the config's
    assert!(project.path().join(format!("rv/library/{mm}")).exists());
    assert!(!project.path().join(format!("rv/library/{other}")).exists());
}

#[test]
#[ignore]
fn sync_combo_with_wrong_r_version_errors() {
    let (r_bin, mm) = r_on_path();
    let other = other_version(&mm);
    let (cache, _project, config) = setup_project(&other);

    rv_sync(
        cache.path(),
        &config,
        &["--r-bin", r_bin.to_str().unwrap(), "--r-version", &other],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("reports version"));
}

#[test]
#[ignore]
fn sync_locked_with_mismatched_combo_errors() {
    let (r_bin, mm) = r_on_path();
    let other = other_version(&mm);
    let (cache, _project, config) = setup_project(&other);

    rv_sync(
        cache.path(),
        &config,
        &[
            "--r-bin",
            r_bin.to_str().unwrap(),
            "--r-version",
            &mm,
            "--locked",
        ],
    )
    .assert()
    .failure()
    .stderr(predicates::str::contains("`--locked` requires the config"));
}

#[test]
#[ignore]
fn sync_r_bin_via_env_var() {
    let (r_bin, mm) = r_on_path();
    let (cache, project, config) = setup_project(&mm);

    rv_sync(cache.path(), &config, &[])
        .env("RV_R_BIN", r_bin)
        .assert()
        .success();

    assert!(project.path().join(format!("rv/library/{mm}")).exists());
}

#[test]
#[ignore]
fn sync_r_bin_flag_overrides_env_var() {
    let (r_bin, mm) = r_on_path();
    let (cache, project, config) = setup_project(&mm);

    // Bogus env var, valid flag: the flag wins and the sync succeeds.
    rv_sync(cache.path(), &config, &["--r-bin", r_bin.to_str().unwrap()])
        .env("RV_R_BIN", project.path().join("no-such-R"))
        .assert()
        .success();
}

#[test]
#[ignore]
fn add_with_mismatched_combo_errors_and_leaves_config_untouched() {
    let (r_bin, mm) = r_on_path();
    let other = other_version(&mm);
    let (cache, project, config) = setup_project(&other);
    let before = fs::read_to_string(&config).unwrap();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.env("RV_CACHE_DIR", cache.path());
    cmd.args([
        "--config-file",
        config.to_str().unwrap(),
        "add",
        "dplyr",
        "--r-bin",
        r_bin.to_str().unwrap(),
        "--r-version",
        &mm,
    ]);
    cmd.assert().failure().stderr(predicates::str::contains(
        "which is not supported by this command",
    ));

    assert_eq!(fs::read_to_string(&config).unwrap(), before);
    let _ = project;
}
