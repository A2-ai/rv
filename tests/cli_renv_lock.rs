use assert_cmd::cargo;
use std::fs;
use tempfile::TempDir;

fn create_test_project() -> (TempDir, std::path::PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rproject.toml");

    let config_content = r#"[project]
name = "test-renv-lock"
r_version = "4.5"
repositories = [
    {alias = "CRAN", url = "https://packagemanager.posit.co/cran/2025-01-01/"},
    {alias = "FakeRepo", url = "https://fake-repo.example.com/packages/"},
]
dependencies = [
    "R6",
    "fakepkg",
    { name = "rv.git.pkgA", git = "https://github.com/A2-ai/rv.git.pkgA", tag = "v0.0.6" },
]
"#;

    let lockfile_content = r#"version = 2
r_version = "4.5"

[[packages]]
name = "R6"
version = "2.6.1"
source = { repository = "https://packagemanager.posit.co/cran/2025-01-01/" }
force_source = false
dependencies = []

[[packages]]
name = "fakepkg"
version = "1.0.0"
source = { repository = "https://fake-repo.example.com/packages/" }
force_source = false
dependencies = []

[[packages]]
name = "rv.git.pkgA"
version = "0.0.5"
source = { git = "https://github.com/A2-ai/rv.git.pkgA", sha = "cbc24e97b857305558ad5a4769086922812627cc", tag = "v0.0.6" }
force_source = true
dependencies = []

[[packages]]
name = "branchpkg"
version = "0.1.0"
source = { git = "https://github.com/org/branchpkg", sha = "aaa111bbb222ccc333ddd444eee555fff666777", branch = "develop" }
force_source = true
dependencies = []

[[packages]]
name = "commitpkg"
version = "0.2.0"
source = { git = "https://github.com/org/commitpkg", sha = "999888777666555444333222111000aaabbbcccd" }
force_source = true
dependencies = []
"#;

    let descriptions: &[(&str, &str)] = &[
        (
            "R6",
            r#"Package: R6
Title: Encapsulated Classes with Reference Semantics
Version: 2.6.1
Authors@R: c(
    person("Winston", "Chang", , "winston@posit.co", role = c("aut", "cre")),
    person("Posit Software, PBC", role = c("cph", "fnd"))
  )
Description: Creates classes with reference semantics.
License: MIT + file LICENSE
URL: https://r6.r-lib.org, https://github.com/r-lib/R6
BugReports: https://github.com/r-lib/R6/issues
Depends: R (>= 3.6)
Suggests: lobstr, testthat (>= 3.0.0)
Encoding: UTF-8
NeedsCompilation: no
Repository: RSPM
"#,
        ),
        (
            "fakepkg",
            // Server-stamped `Repository: SomeServer` — must be overridden
            // by the config alias "FakeRepo" in the renv.lock output.
            r#"Package: fakepkg
Title: A Fake Package For Testing Repository Override
Version: 1.0.0
Description: Not a real package.
License: MIT + file LICENSE
Encoding: UTF-8
Repository: SomeServer
"#,
        ),
        (
            "rv.git.pkgA",
            "Package: rv.git.pkgA\nTitle: Package Which Has No Dependencies\nVersion: 0.0.5\nLicense: MIT + file LICENSE\nEncoding: UTF-8\n",
        ),
        (
            "branchpkg",
            "Package: branchpkg\nTitle: Branch Test Package\nVersion: 0.1.0\nLicense: MIT + file LICENSE\nEncoding: UTF-8\n",
        ),
        (
            "commitpkg",
            "Package: commitpkg\nTitle: Commit Test Package\nVersion: 0.2.0\nLicense: MIT + file LICENSE\nEncoding: UTF-8\n",
        ),
    ];

    fs::write(&config_path, config_content).unwrap();
    fs::write(temp_dir.path().join("rv.lock"), lockfile_content).unwrap();

    // Build library at the path Context expects: rv/library/{r_version}/{arch}/{codename}/
    let lib_base = temp_dir.path().join("rv").join("library");
    let arch_str = std::env::consts::ARCH;
    let codename = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("VERSION_CODENAME="))
                .map(|l| l.trim_start_matches("VERSION_CODENAME=").to_string())
        })
        .unwrap_or_else(|| "noble".to_string());

    let lib_path = lib_base.join("4.5").join(arch_str).join(&codename);
    for (name, desc) in descriptions {
        fs::create_dir_all(lib_path.join(name)).unwrap();
        fs::write(lib_path.join(name).join("DESCRIPTION"), desc).unwrap();
    }

    (temp_dir, config_path)
}

fn run_renv_lock(
    config_path: &std::path::Path,
    output_path: &std::path::Path,
) -> serde_json::Value {
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "renv",
        "lock",
        "--output",
        output_path.to_str().unwrap(),
        "--config-file",
        config_path.to_str().unwrap(),
    ]);
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Command failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let content = fs::read_to_string(output_path).unwrap();
    serde_json::from_str(&content).expect("valid JSON in renv.lock")
}

fn assert_github_pkg(
    pkg: &serde_json::Value,
    username: &str,
    repo: &str,
    remote_ref: &str,
    sha: &str,
) {
    assert_eq!(pkg["Source"], "GitHub");
    assert_eq!(pkg["RemoteType"], "github");
    assert_eq!(pkg["RemoteHost"], "api.github.com");
    assert_eq!(pkg["RemoteUsername"], username);
    assert_eq!(pkg["RemoteRepo"], repo);
    assert_eq!(pkg["RemoteRef"], remote_ref);
    assert_eq!(pkg["RemoteSha"], sha);
}

#[test]
fn test_renv_lock_generation() {
    let (temp_dir, config_path) = create_test_project();
    let output_path = temp_dir.path().join("renv.lock");
    let renv_lock = run_renv_lock(&config_path, &output_path);

    // R section
    assert_eq!(renv_lock["R"]["Version"], "4.5");
    assert_eq!(renv_lock["R"]["Repositories"][0]["Name"], "CRAN");

    // Repository source — R6's DESCRIPTION has `Repository: RSPM` (server-stamped by
    // Posit Package Manager), but the config alias is "CRAN". The output must use the
    // config alias, not the server-stamped value.
    let r6 = &renv_lock["Packages"]["R6"];
    assert_eq!(r6["Source"], "Repository");
    assert_eq!(r6["Repository"], "CRAN");
    assert_ne!(r6["Repository"], "RSPM", "must not leak server-stamped Repository value");
    assert_eq!(r6["Depends"], serde_json::json!(["R (>= 3.6)"]));
    assert_eq!(
        r6["Suggests"],
        serde_json::json!(["lobstr", "testthat (>= 3.0.0)"])
    );

    // Fake package — DESCRIPTION has `Repository: SomeServer` (server-stamped),
    // but the config alias is "FakeRepo". Verifies the override works for any
    // server-stamped value, not just RSPM.
    let fakepkg = &renv_lock["Packages"]["fakepkg"];
    assert_eq!(fakepkg["Source"], "Repository");
    assert_eq!(fakepkg["Repository"], "FakeRepo");
    assert_ne!(fakepkg["Repository"], "SomeServer", "must not leak server-stamped Repository value");

    // Git source — tag: verify both remote metadata and carried-through DESCRIPTION fields
    let git_pkg = &renv_lock["Packages"]["rv.git.pkgA"];
    assert_github_pkg(
        git_pkg,
        "A2-ai",
        "rv.git.pkgA",
        "v0.0.6",
        "cbc24e97b857305558ad5a4769086922812627cc",
    );
    assert_eq!(git_pkg["Title"], "Package Which Has No Dependencies");
    assert_eq!(git_pkg["License"], "MIT + file LICENSE");
    assert_eq!(git_pkg["Encoding"], "UTF-8");

    // Git source — branch
    assert_github_pkg(
        &renv_lock["Packages"]["branchpkg"],
        "org",
        "branchpkg",
        "develop",
        "aaa111bbb222ccc333ddd444eee555fff666777",
    );

    // Git source — commit only (RemoteRef falls back to SHA)
    assert_github_pkg(
        &renv_lock["Packages"]["commitpkg"],
        "org",
        "commitpkg",
        "999888777666555444333222111000aaabbbcccd",
        "999888777666555444333222111000aaabbbcccd",
    );
}

#[test]
fn test_renv_lock_json_output() {
    let (temp_dir, config_path) = create_test_project();
    let output_path = temp_dir.path().join("renv.lock");

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "renv",
        "lock",
        "--output",
        output_path.to_str().unwrap(),
        "--config-file",
        config_path.to_str().unwrap(),
        "--json",
    ]);

    let output = cmd.output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_output: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json_output["output"].as_str().is_some());
}

// --- Exclusion tests ---
//
// Dependency graph:
//   top-level: applib, devtools, linter
//   applib   → sharedutil, corelib
//   devtools → testhelper, sharedutil
//   linter   → lintcore
//
// This tests real-ish scenarios where dev packages share deps with production packages.

fn create_exclusion_test_project() -> (TempDir, std::path::PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rproject.toml");

    let config_content = r#"[project]
name = "test-exclusion"
r_version = "4.5"
repositories = [
    {alias = "CRAN", url = "https://cran.example.com/"}
]
dependencies = ["applib", "devtools", "linter"]
"#;

    let lockfile_content = r#"version = 2
r_version = "4.5"

[[packages]]
name = "applib"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = ["sharedutil", "corelib"]

[[packages]]
name = "devtools"
version = "2.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = ["testhelper", "sharedutil"]

[[packages]]
name = "linter"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = ["lintcore"]

[[packages]]
name = "sharedutil"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = []

[[packages]]
name = "corelib"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = []

[[packages]]
name = "testhelper"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = []

[[packages]]
name = "lintcore"
version = "1.0.0"
source = { repository = "https://cran.example.com/" }
force_source = false
dependencies = []
"#;

    let descriptions: &[(&str, &str)] = &[
        ("applib", "Package: applib\nVersion: 1.0.0\nTitle: App Library\nLicense: MIT\nEncoding: UTF-8\n"),
        ("devtools", "Package: devtools\nVersion: 2.0.0\nTitle: Dev Tools\nLicense: MIT\nEncoding: UTF-8\n"),
        ("linter", "Package: linter\nVersion: 1.0.0\nTitle: Linter\nLicense: MIT\nEncoding: UTF-8\n"),
        ("sharedutil", "Package: sharedutil\nVersion: 1.0.0\nTitle: Shared Utilities\nLicense: MIT\nEncoding: UTF-8\n"),
        ("corelib", "Package: corelib\nVersion: 1.0.0\nTitle: Core Library\nLicense: MIT\nEncoding: UTF-8\n"),
        ("testhelper", "Package: testhelper\nVersion: 1.0.0\nTitle: Test Helper\nLicense: MIT\nEncoding: UTF-8\n"),
        ("lintcore", "Package: lintcore\nVersion: 1.0.0\nTitle: Lint Core\nLicense: MIT\nEncoding: UTF-8\n"),
    ];

    fs::write(&config_path, config_content).unwrap();
    fs::write(temp_dir.path().join("rv.lock"), lockfile_content).unwrap();

    let lib_base = temp_dir.path().join("rv").join("library");
    let arch_str = std::env::consts::ARCH;
    let codename = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("VERSION_CODENAME="))
                .map(|l| l.trim_start_matches("VERSION_CODENAME=").to_string())
        })
        .unwrap_or_else(|| "noble".to_string());

    let lib_path = lib_base.join("4.5").join(arch_str).join(&codename);
    for (name, desc) in descriptions {
        fs::create_dir_all(lib_path.join(name)).unwrap();
        fs::write(lib_path.join(name).join("DESCRIPTION"), desc).unwrap();
    }

    (temp_dir, config_path)
}

fn run_renv_lock_with_excludes(
    config_path: &std::path::Path,
    output_path: &std::path::Path,
    exclude_pkgs: &str,
) -> serde_json::Value {
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "renv",
        "lock",
        "--output",
        output_path.to_str().unwrap(),
        "--config-file",
        config_path.to_str().unwrap(),
        "--exclude-pkgs",
        exclude_pkgs,
    ]);
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Command failed.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let content = fs::read_to_string(output_path).unwrap();
    serde_json::from_str(&content).expect("valid JSON in renv.lock")
}

#[test]
fn test_exclude_single_pkg_removes_exclusive_deps() {
    let (temp_dir, config_path) = create_exclusion_test_project();
    let output_path = temp_dir.path().join("renv.lock");
    let renv_lock = run_renv_lock_with_excludes(&config_path, &output_path, "devtools");

    let packages = renv_lock["Packages"].as_object().unwrap();

    // devtools and testhelper (exclusive to devtools) should be gone
    assert!(!packages.contains_key("devtools"), "devtools should be excluded");
    assert!(!packages.contains_key("testhelper"), "testhelper should be excluded (only needed by devtools)");

    // sharedutil is used by applib — must be retained
    assert!(packages.contains_key("sharedutil"), "sharedutil should be retained (needed by applib)");

    // All other packages should remain
    assert!(packages.contains_key("applib"));
    assert!(packages.contains_key("linter"));
    assert!(packages.contains_key("corelib"));
    assert!(packages.contains_key("lintcore"));
}

#[test]
fn test_exclude_multiple_pkgs() {
    let (temp_dir, config_path) = create_exclusion_test_project();
    let output_path = temp_dir.path().join("renv.lock");
    let renv_lock = run_renv_lock_with_excludes(&config_path, &output_path, "devtools,linter");

    let packages = renv_lock["Packages"].as_object().unwrap();

    // devtools, testhelper, linter, lintcore should all be gone
    assert!(!packages.contains_key("devtools"));
    assert!(!packages.contains_key("testhelper"));
    assert!(!packages.contains_key("linter"));
    assert!(!packages.contains_key("lintcore"));

    // applib and its deps should remain
    assert!(packages.contains_key("applib"));
    assert!(packages.contains_key("sharedutil"));
    assert!(packages.contains_key("corelib"));
}

#[test]
fn test_exclude_non_top_level_fails() {
    let (temp_dir, config_path) = create_exclusion_test_project();
    let output_path = temp_dir.path().join("renv.lock");

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "renv",
        "lock",
        "--output",
        output_path.to_str().unwrap(),
        "--config-file",
        config_path.to_str().unwrap(),
        "--exclude-pkgs",
        "testhelper",
    ]);
    let output = cmd.output().unwrap();
    assert!(
        !output.status.success(),
        "Should fail when excluding non-top-level dep"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not a top-level dependency"),
        "Error should mention top-level dependency. Got: {stderr}"
    );
}

#[test]
fn test_dry_run_shows_exclusion_report() {
    let (temp_dir, config_path) = create_exclusion_test_project();
    let output_path = temp_dir.path().join("renv.lock");

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "renv",
        "lock",
        "--output",
        output_path.to_str().unwrap(),
        "--config-file",
        config_path.to_str().unwrap(),
        "--exclude-pkgs",
        "devtools",
        "--dry-run",
    ]);
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Dry run failed.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Should show devtools as directly excluded
    assert!(stdout.contains("devtools"), "Should mention devtools in output");
    // Should show testhelper as transitively removed
    assert!(stdout.contains("testhelper"), "Should mention testhelper as transitively removed");
    // Should show sharedutil as retained
    assert!(stdout.contains("sharedutil"), "Should mention sharedutil as retained");
    // Should NOT have written the file
    assert!(!output_path.exists(), "Dry run should not write the file");
}

#[test]
fn test_dry_run_json_output() {
    let (temp_dir, config_path) = create_exclusion_test_project();
    let output_path = temp_dir.path().join("renv.lock");

    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.args([
        "renv",
        "lock",
        "--output",
        output_path.to_str().unwrap(),
        "--config-file",
        config_path.to_str().unwrap(),
        "--exclude-pkgs",
        "devtools",
        "--dry-run",
        "--json",
    ]);
    let output = cmd.output().unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json_output: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("Expected valid JSON, got: {stdout}"));
    assert_eq!(json_output["directly_excluded"], serde_json::json!(["devtools"]));
    assert_eq!(json_output["transitively_removed"], serde_json::json!(["testhelper"]));
    assert_eq!(json_output["excluded_count"], 2);
}
