use assert_cmd::cargo;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn create_project(sandbox: bool) -> (TempDir, TempDir, std::path::PathBuf) {
    let project = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let config = project.path().join("rproject.toml");
    fs::write(
        &config,
        format!(
            r#"[project]
name = "test-run-isolated"
r_version = "4.5"
sandbox = {sandbox}
repositories = []
dependencies = []
"#
        ),
    )
    .unwrap();
    (project, cache, config)
}

fn rv_cmd(cache: &TempDir, config: &std::path::Path) -> assert_cmd::Command {
    let mut command = cargo::cargo_bin_cmd!();
    command
        .env("RV_CACHE_DIR", cache.path())
        .args(["--config-file", config.to_str().unwrap()]);
    command
}

fn probe_values(output: &str) -> HashMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            line.split_once('\t')
                .map(|(key, value)| (key.to_string(), value.to_string()))
        })
        .collect()
}

#[test]
fn rv_run_uses_sandbox_and_loads_user_profile_by_default() {
    let (project, cache, config) = create_project(true);
    let profile = project.path().join("host.Rprofile");
    fs::write(
        &profile,
        "Sys.setenv(RV_EXPLICIT_PROFILE = 'profile-loaded')\n",
    )
    .unwrap();
    let environ = project.path().join("host.Renviron");
    fs::write(&environ, "RV_EXPLICIT_ENVIRON=environ-loaded\n").unwrap();

    let mut command = rv_cmd(&cache, &config);
    command
        .current_dir(project.path())
        .env("R_PROFILE_USER", &profile)
        .env("R_ENVIRON_USER", &environ)
        .args([
            "run",
            "--no-sync",
            "-e",
            r#"cat(
                paste("library", .Library, sep = "\t"),
                paste("paths", paste(.libPaths(), collapse = .Platform$path.sep), sep = "\t"),
                paste("profile", Sys.getenv("RV_EXPLICIT_PROFILE"), sep = "\t"),
                paste("environ", Sys.getenv("RV_EXPLICIT_ENVIRON"), sep = "\t"),
                sep = "\n"
            )"#,
        ]);

    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let observed = probe_values(&stdout);
    assert!(observed["library"].contains("sandboxes"), "{stdout}");
    assert!(observed["paths"].contains("rv/library"), "{stdout}");
    assert!(observed["paths"].contains("sandboxes"), "{stdout}");
    assert_eq!(observed["profile"], "profile-loaded", "{stdout}");
    assert_eq!(observed["environ"], "environ-loaded", "{stdout}");
}
