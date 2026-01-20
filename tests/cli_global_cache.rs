use assert_cmd::{Command, cargo};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Create a test project with R6 and a git dependency
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
    { name = "rv.git.pkgA", git = "https://github.com/A2-ai/rv.git.pkgA", branch = "main", install_suggestions = true },
]
"#;

    fs::write(&config_path, config_content).unwrap();
    (temp_dir, config_path)
}

fn rv_cmd(local_cache: &Path, global_cache: Option<&Path>, config: &Path) -> Command {
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.env("RV_CACHE_DIR", local_cache);
    if let Some(global) = global_cache {
        cmd.env("RV_GLOBAL_CACHE_DIR", global);
    }
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd
}

fn count_files_in_dir(dir: &Path, exclude_metadata: bool) -> usize {
    if !dir.exists() {
        return 0;
    }
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| !e.path().components().any(|c| c.as_os_str() == ".git"))
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            if !exclude_metadata {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            println!("{name:?}");
            name != "packages.mp"
                && name != "CACHEDIR.TAG"
                && !name.starts_with("builtin-")
                && !name.starts_with("sysreq-")
                && !name.ends_with(".mp")
        })
        .count()
}

/// Find a package directory in the library (handles nested R version/arch structure)
fn find_package_in_library(project_dir: &Path, pkg_name: &str) -> Option<PathBuf> {
    let library_root = project_dir.join("rv/library");
    if !library_root.exists() {
        return None;
    }
    for entry in walkdir::WalkDir::new(&library_root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_name().to_string_lossy() == pkg_name && entry.file_type().is_dir() {
            // Verify it's a valid R package (has DESCRIPTION file)
            if entry.path().join("DESCRIPTION").exists() {
                return Some(entry.path().to_path_buf());
            }
        }
    }
    None
}

#[test]
fn test_cache_with_global_set() {
    let local_cache = TempDir::new().unwrap();
    let global_cache = TempDir::new().unwrap();
    let (_temp_dir, config_path) = create_test_project();

    // Test text output
    let mut cmd = rv_cmd(local_cache.path(), Some(global_cache.path()), &config_path);
    cmd.args(["cache"]);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("Local:"));
    assert!(stdout.contains("Global:"));

    // Test JSON output
    let mut cmd = rv_cmd(local_cache.path(), Some(global_cache.path()), &config_path);
    cmd.args(["--json", "cache"]);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json.get("local_info").is_some());
    assert!(json.get("global_info").unwrap().is_object());
}

#[test]
fn test_invalid_global_cache_path() {
    let local_cache = TempDir::new().unwrap();
    let (_temp_dir, config_path) = create_test_project();

    let invalid_path = Path::new("/nonexistent/path/that/does/not/exist");
    let mut cmd = rv_cmd(local_cache.path(), Some(invalid_path), &config_path);
    cmd.args(["cache"]);

    let output = cmd.output().unwrap();

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Local:"));
    assert!(!stdout.contains("Global:"));
}

#[test]
fn test_summary_with_global_set() {
    let local_cache = TempDir::new().unwrap();
    let global_cache = TempDir::new().unwrap();
    let (_temp_dir, config_path) = create_test_project();

    // Test text output
    let mut cmd = rv_cmd(local_cache.path(), Some(global_cache.path()), &config_path);
    cmd.args(["summary"]);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(stdout.contains("Global Cache Location"));

    // Test JSON output
    let mut cmd = rv_cmd(local_cache.path(), Some(global_cache.path()), &config_path);
    cmd.args(["--json", "summary"]);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json.get("global_cache_root").is_some());
    assert!(!json.get("global_cache_root").unwrap().is_null());
}

// Test is ignored locally because it can take 10s
#[test]
#[ignore]
fn test_local_untouched_if_pkg_found_in_global() {
    let local_cache = TempDir::new().unwrap();
    let global_cache = TempDir::new().unwrap();

    // first install it in the global cache
    let (temp_dir, config_path) = create_test_project();
    let mut cmd = rv_cmd(global_cache.path(), None, &config_path);
    cmd.args(["sync"]);
    cmd.assert().success();
    assert_eq!(count_files_in_dir(local_cache.path(), false), 0);
    // then install it using the global cache
    let mut cmd = rv_cmd(local_cache.path(), Some(global_cache.path()), &config_path);
    cmd.args(["sync"]);
    cmd.assert().success();

    // We will have 1 file, DESCRIPTION of the git dep since we need that for resolution
    // We will not have any other file as after reading it, rv will see it can get what it needs
    // from the cache
    assert_eq!(count_files_in_dir(local_cache.path(), true), 1);
    // Verify both packages are installed in the library
    let found_r6 = find_package_in_library(temp_dir.path(), "R6");
    assert!(found_r6.is_some(), "R6 should be installed in the library");
    let found_git_pkg = find_package_in_library(temp_dir.path(), "rv.git.pkgA");
    assert!(
        found_git_pkg.is_some(),
        "rv.git.pkgA should be installed in the library"
    );
}
