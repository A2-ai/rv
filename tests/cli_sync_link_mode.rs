#[cfg(unix)]
use assert_cmd::cargo;

#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
fn rv_sync_symlink(cache: &Path, config: &Path) {
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.env("RV_CACHE_DIR", cache);
    cmd.env("RV_LINK_MODE", "symlink");
    cmd.args(["--config-file", config.to_str().unwrap(), "sync"]);
    cmd.assert().success();
}

#[cfg(unix)]
#[test]
#[ignore]
fn sync_with_symlink_mode_populates_library() {
    use std::fs;
    use tempfile::TempDir;

    let cache = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let config = project.path().join("rproject.toml");
    fs::write(
        &config,
        r#"[project]
name = "test"
r_version = "4.5"
repositories = [
    {alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}
]
dependencies = ["R6"]
"#,
    )
    .unwrap();

    // The first sync may build R6 from source (a real directory in the cache).
    rv_sync_symlink(cache.path(), &config);

    // Wipe the library and sync again: R6 is now served from the cache and linked
    // in, so symlink mode produces a symlink in the library.
    fs::remove_dir_all(project.path().join("rv/library")).unwrap();
    rv_sync_symlink(cache.path(), &config);

    // Find the R6 entry under rv/library (the path nests r-version/arch[/os]).
    // walkdir does not follow symlinks, so a symlinked package is the symlink itself.
    let r6 = walkdir::WalkDir::new(project.path().join("rv/library"))
        .into_iter()
        .filter_map(|e| e.ok())
        .find(|e| e.file_name() == "R6")
        .map(|e| e.into_path())
        .expect("R6 must be installed in the library (empty library => regression)");

    assert!(
        fs::symlink_metadata(&r6).unwrap().file_type().is_symlink(),
        "R6 should be a symlink into the cache in symlink mode, got {r6:?}"
    );
    assert!(
        r6.join("DESCRIPTION").exists(),
        "the R6 symlink should resolve to a real package"
    );
}
