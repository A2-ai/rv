use assert_cmd::cargo;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Normalize path separators to forward slashes for cross-platform comparison.
/// rv outputs forward slashes on Windows, but TempDir/PathBuf uses backslashes.
fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
}

fn create_test_project(library_field: Option<&str>) -> (TempDir, std::path::PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("rproject.toml");

    let library_line = match library_field {
        Some(lib) => format!("library = \"{lib}\"\n\n"),
        None => String::new(),
    };

    // Need to escape braces for the toml array when using format!
    let config_content = format!(
        r#"{library_line}[project]
name = "test"
r_version = "4.5"
repositories = [
    {{alias = "posit", url = "https://packagemanager.posit.co/cran/2024-12-16/"}}
]
dependencies = []
"#
    );

    fs::write(&config_path, config_content).unwrap();
    (temp_dir, config_path)
}

fn rv_cmd(cache: &Path, config: &Path) -> assert_cmd::Command {
    let mut cmd = cargo::cargo_bin_cmd!();
    cmd.env("RV_CACHE_DIR", cache);
    cmd.args(["--config-file", config.to_str().unwrap()]);
    cmd
}

fn get_library_path(cache: &Path, config: &Path, env_var: Option<&str>) -> String {
    let mut cmd = rv_cmd(cache, config);
    if let Some(val) = env_var {
        cmd.env("RV_LIBRARY_DIR", val);
    } else {
        cmd.env_remove("RV_LIBRARY_DIR");
    }
    cmd.arg("library");

    let output = cmd.output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn test_default_library_path() {
    let cache = TempDir::new().unwrap();
    let (temp_dir, config_path) = create_test_project(None);

    let library_path = get_library_path(cache.path(), &config_path, None);
    let project_dir = normalize_path(&temp_dir.path().to_string_lossy());

    assert!(
        library_path.starts_with(&project_dir),
        "library path should be under project dir, got: {library_path}"
    );
    assert!(
        library_path.contains("rv/library"),
        "default library path should contain rv/library, got: {library_path}"
    );
}

/// Tests for RV_LIBRARY_DIR env var and config file `library` field interactions.
///
/// Each case: (description, config library field, env var, expected path suffix or absolute)
#[test]
fn test_library_dir_resolution() {
    struct Case {
        name: &'static str,
        config_library: Option<&'static str>,
        env_var: Option<&'static str>,
        /// If true, the expected path is the env var value used as an absolute path (via a temp dir).
        /// If false, expected is project_dir.join(expected_suffix).
        expect_absolute_env: bool,
        expected_suffix: &'static str,
    }

    let cases = [
        Case {
            name: "env var relative path resolves against project dir",
            config_library: None,
            env_var: Some("my-libs"),
            expect_absolute_env: false,
            expected_suffix: "my-libs",
        },
        Case {
            name: "env var overrides config library field",
            config_library: Some("libs"),
            env_var: Some("__ABSOLUTE__"),
            expect_absolute_env: true,
            expected_suffix: "",
        },
        Case {
            name: "config library field used when env var not set",
            config_library: Some("libs"),
            env_var: None,
            expect_absolute_env: false,
            expected_suffix: "libs",
        },
        Case {
            name: "env var absolute path used directly",
            config_library: None,
            env_var: Some("__ABSOLUTE__"),
            expect_absolute_env: true,
            expected_suffix: "",
        },
    ];

    for case in &cases {
        let cache = TempDir::new().unwrap();
        let (temp_dir, config_path) = create_test_project(case.config_library);

        // For absolute path tests, create a real temp dir to use as the env var value
        let abs_dir = TempDir::new().unwrap();
        let env_val = match case.env_var {
            Some("__ABSOLUTE__") => Some(abs_dir.path().to_str().unwrap().to_string()),
            Some(v) => Some(v.to_string()),
            None => None,
        };

        let library_path = get_library_path(cache.path(), &config_path, env_val.as_deref());

        let expected = if case.expect_absolute_env {
            normalize_path(&abs_dir.path().to_string_lossy())
        } else {
            normalize_path(&temp_dir.path().join(case.expected_suffix).to_string_lossy())
        };

        assert_eq!(library_path, expected, "FAILED: {}", case.name);
    }
}

/// The activate.R script calls `rv info --library` at R startup to determine
/// the library path. Verify that this command respects RV_LIBRARY_DIR.
#[test]
fn test_info_library_respects_env_var() {
    let cache = TempDir::new().unwrap();
    let (_temp_dir, config_path) = create_test_project(None);
    let custom_lib = TempDir::new().unwrap();

    let mut cmd = rv_cmd(cache.path(), &config_path);
    cmd.env("RV_LIBRARY_DIR", custom_lib.path());
    cmd.args(["info", "--library"]);

    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // rv info --library outputs "library: <path>"
    let library_path = stdout
        .trim()
        .strip_prefix("library: ")
        .expect("expected 'library: ' prefix in rv info output");

    assert_eq!(
        library_path,
        normalize_path(&custom_lib.path().to_string_lossy()),
        "rv info --library should respect RV_LIBRARY_DIR during activation"
    );
}
