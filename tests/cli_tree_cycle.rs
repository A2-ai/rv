use assert_cmd::cargo;
use rv::{Cache, RepositoryDatabase, SystemInfo, Version};
use std::fs;
use tempfile::TempDir;

#[test]
fn tree_handles_self_cycle_from_suggests() {
    let project_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();

    let repo_url = "https://cycle.test/repo";
    let cli_r_version: Version = "99.0".parse().unwrap();

    let cache =
        Cache::new_in_dir(&cli_r_version, SystemInfo::from_os_info(), cache_dir.path()).unwrap();

    let (repo_db_path, _) = cache.local().get_package_db_entry(repo_url);
    let mut db = RepositoryDatabase::new(repo_url);
    db.parse_source(
        r#"Package: covr
Version: 3.6.5
Depends: R (>= 4.1)
Suggests: covr
NeedsCompilation: no
License: MIT + file LICENSE
"#,
    );
    db.persist(&repo_db_path).unwrap();

    let config_path = project_dir.path().join("rproject.toml");
    fs::write(
        &config_path,
        format!(
            r#"use_lockfile = false

[project]
name = "cycle"
r_version = "4.5"
repositories = [
  {{ alias = "local", url = "{repo_url}" }}
]
dependencies = [
  {{ name = "covr", install_suggestions = true }}
]
"#
        ),
    )
    .unwrap();

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.env("RV_CACHE_DIR", cache_dir.path()).args([
        "--config-file",
        config_path.to_str().unwrap(),
        "tree",
        "--hide-system-deps",
        "--r-version",
        "99.0",
    ]);

    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
