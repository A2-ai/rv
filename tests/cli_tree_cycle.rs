use assert_cmd::cargo;
use rv::{Cache, RepositoryDatabase, SystemInfo, Version};
use std::fs;
use tempfile::TempDir;

fn diamond_project() -> (TempDir, TempDir) {
    let project_dir = TempDir::new().unwrap();
    let cache_dir = TempDir::new().unwrap();

    let repo_url = "https://diamond.test/repo";
    let cli_r_version: Version = "99.0".parse().unwrap();

    let cache =
        Cache::new_in_dir(&cli_r_version, SystemInfo::from_os_info(), cache_dir.path()).unwrap();

    let (repo_db_path, _) = cache.local().get_package_db_entry(repo_url);
    let mut db = RepositoryDatabase::new(repo_url);
    // Diamond: a -> {b, c}; b -> d; c -> d; d -> e.
    // d having a child lets us tell dedup from no-dedup by counting `e` lines.
    db.parse_source(
        r#"Package: a
Version: 1.0.0
Depends: R (>= 4.1), b, c
NeedsCompilation: no
License: MIT + file LICENSE

Package: b
Version: 1.0.0
Depends: R (>= 4.1), d
NeedsCompilation: no
License: MIT + file LICENSE

Package: c
Version: 1.0.0
Depends: R (>= 4.1), d
NeedsCompilation: no
License: MIT + file LICENSE

Package: d
Version: 1.0.0
Depends: R (>= 4.1), e
NeedsCompilation: no
License: MIT + file LICENSE

Package: e
Version: 1.0.0
Depends: R (>= 4.1)
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
name = "diamond"
r_version = "4.5"
repositories = [
  {{ alias = "local", url = "{repo_url}" }}
]
dependencies = [
  {{ name = "a" }}
]
"#
        ),
    )
    .unwrap();

    (project_dir, cache_dir)
}

#[test]
fn tree_dedupes_diamond_dependencies() {
    let (project_dir, cache_dir) = diamond_project();
    let config_path = project_dir.path().join("rproject.toml");

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
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    insta::assert_snapshot!("diamond_dedup_stdout", stdout);
}

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
