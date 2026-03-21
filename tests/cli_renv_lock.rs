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
    {alias = "CRAN", url = "https://packagemanager.posit.co/cran/2025-01-01/"}
]
dependencies = [
    "R6",
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

    // Repository source
    let r6 = &renv_lock["Packages"]["R6"];
    assert_eq!(r6["Source"], "Repository");
    assert_eq!(r6["Repository"], "CRAN");
    assert_eq!(r6["Depends"], serde_json::json!(["R (>= 3.6)"]));
    assert_eq!(
        r6["Suggests"],
        serde_json::json!(["lobstr", "testthat (>= 3.0.0)"])
    );

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
