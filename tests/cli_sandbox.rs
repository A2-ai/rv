use assert_cmd::cargo;
use std::collections::HashMap;
use std::fs;
use tempfile::TempDir;

fn create_project(sandbox: Option<bool>) -> (TempDir, TempDir, std::path::PathBuf) {
    let project = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let config = project.path().join("rproject.toml");
    let sandbox = sandbox
        .map(|value| format!("sandbox = {value}\n"))
        .unwrap_or_default();
    fs::write(
        &config,
        format!(
            r#"[project]
name = "test-sandbox"
r_version = "4.5"
{sandbox}repositories = []
dependencies = []
"#
        ),
    )
    .unwrap();
    (project, cache, config)
}

fn rv_cmd(cache: &TempDir, config: &std::path::Path) -> assert_cmd::Command {
    let mut command = cargo::cargo_bin_cmd!();
    command.env("RV_CACHE_DIR", cache.path());
    command.args(["--config-file", config.to_str().unwrap()]);
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
fn environment_enables_sandbox_when_config_is_unset() {
    let (_project, cache, config) = create_project(None);
    let mut command = rv_cmd(&cache, &config);
    command.env("RV_USE_SANDBOX", "1");
    command.args(["info", "--sandbox"]);

    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sandbox = String::from_utf8_lossy(&output.stdout)
        .trim()
        .strip_prefix("sandbox: ")
        .unwrap()
        .to_string();
    assert!(!sandbox.is_empty());
    assert!(std::path::Path::new(&sandbox).is_dir());
}

#[test]
fn explicit_config_disables_environment_override() {
    let (_project, cache, config) = create_project(Some(false));
    let mut command = rv_cmd(&cache, &config);
    command.env("RV_USE_SANDBOX", "1");
    command.args(["info", "--sandbox"]);

    let output = command.output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "sandbox:");
}

#[test]
fn explicit_config_enables_sandbox_when_environment_is_false() {
    let (_project, cache, config) = create_project(Some(true));
    let mut command = rv_cmd(&cache, &config);
    command.env("RV_USE_SANDBOX", "0");
    command.args(["info", "--sandbox"]);

    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sandbox = String::from_utf8_lossy(&output.stdout)
        .trim()
        .strip_prefix("sandbox: ")
        .unwrap()
        .to_string();
    assert!(!sandbox.is_empty());
    assert!(std::path::Path::new(&sandbox).is_dir());
}

#[test]
fn rv_run_uses_sandbox_without_project_activation_profile() {
    let (project, cache, config) = create_project(Some(true));
    assert!(!project.path().join(".Rprofile").exists());

    let mut command = rv_cmd(&cache, &config);
    command.current_dir(project.path());
    command.args([
        "run",
        "--no-sync",
        "-e",
        r#"cat(.Library, Sys.getenv("R_LIBS_SITE"), sep = "\n")"#,
    ]);

    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2, "unexpected output: {stdout}");
    assert!(
        lines[0].contains("sandboxes"),
        "unexpected .Library: {stdout}"
    );
    assert!(
        lines[1].contains("rv/library") || lines[1].contains("rv\\library"),
        "project library missing: {stdout}"
    );
    assert!(
        lines[1].contains("sandboxes"),
        "sandbox missing from R_LIBS_SITE: {stdout}"
    );
}

#[test]
fn rv_run_uses_sandbox_and_loads_user_profile_by_default() {
    let (project, cache, config) = create_project(Some(true));
    let profile = project.path().join("host.Rprofile");
    fs::write(
        &profile,
        "Sys.setenv(RV_EXPLICIT_PROFILE = 'profile-loaded')\n",
    )
    .unwrap();

    let mut command = rv_cmd(&cache, &config);
    command
        .current_dir(project.path())
        .env("R_PROFILE_USER", &profile)
        .args([
            "run",
            "--no-sync",
            "-e",
            r#"cat(paste("library", .Library, sep = "\t"), paste("profile", Sys.getenv("RV_EXPLICIT_PROFILE"), sep = "\t"), sep = "\n")"#,
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
    assert_eq!(observed["profile"], "profile-loaded", "{stdout}");
}

#[test]
fn sync_isolates_r_build_and_install_subprocesses() {
    let project = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let probe = project.path().join("probe");
    fs::create_dir_all(probe.join("R")).unwrap();
    fs::write(
        probe.join("DESCRIPTION"),
        r#"Package: sandboxprobe
Version: 0.1.0
Title: Verify rv Sandbox Isolation
Description: Records the R library paths observed while installing.
Authors@R: person("rv", "test", email = "rv@example.com", role = c("aut", "cre"))
License: MIT
Encoding: UTF-8
"#,
    )
    .unwrap();
    fs::write(probe.join("NAMESPACE"), "export(probe)\n").unwrap();
    fs::write(
        probe.join("R").join("probe.R"),
        r#"local({
    values <- c(
        library = .Library,
        site = paste(.Library.site, collapse = .Platform$path.sep),
        paths = paste(.libPaths(), collapse = .Platform$path.sep),
        r_libs = Sys.getenv("R_LIBS"),
        r_libs_site = Sys.getenv("R_LIBS_SITE"),
        r_libs_user = Sys.getenv("R_LIBS_USER"),
        r_environ = Sys.getenv("R_ENVIRON"),
        r_environ_user = Sys.getenv("R_ENVIRON_USER"),
        r_profile = Sys.getenv("R_PROFILE"),
        r_profile_user = Sys.getenv("R_PROFILE_USER"),
        environ_leak = Sys.getenv("RV_HOST_ENVIRON_LEAK"),
        profile_leak = Sys.getenv("RV_HOST_PROFILE_LEAK")
    )
    writeLines(paste(names(values), values, sep = "\t"), Sys.getenv("RV_TEST_OUTPUT"))
})
probe <- function() TRUE
"#,
    )
    .unwrap();

    let observed = project.path().join("observed-library-paths.txt");
    let observed_for_toml = observed.to_string_lossy().replace('\\', "/");
    let hostile_library = project.path().join("host-library");
    fs::create_dir(&hostile_library).unwrap();
    let hostile_library_for_toml = hostile_library.to_string_lossy().replace('\\', "/");
    let hostile_environ = project.path().join("host.Renviron");
    fs::write(&hostile_environ, "RV_HOST_ENVIRON_LEAK=environ-loaded\n").unwrap();
    let hostile_environ_for_toml = hostile_environ.to_string_lossy().replace('\\', "/");
    let hostile_profile = project.path().join("host.Rprofile");
    fs::write(
        &hostile_profile,
        "Sys.setenv(RV_HOST_PROFILE_LEAK = 'profile-loaded')\n",
    )
    .unwrap();
    let hostile_profile_for_toml = hostile_profile.to_string_lossy().replace('\\', "/");
    let config = project.path().join("rproject.toml");
    fs::write(
        &config,
        format!(
            r#"[project]
name = "test-install-sandbox"
r_version = "4.5"
sandbox = true
repositories = []
dependencies = [{{ name = "sandboxprobe", path = "probe" }}]

[project.packages_env_vars]
sandboxprobe = {{
    RV_TEST_OUTPUT = "{observed_for_toml}",
    RV_SANDBOX_LIBRARY = "{hostile_library_for_toml}",
    R_LIBS = "{hostile_library_for_toml}",
    R_LIBS_SITE = "{hostile_library_for_toml}",
    R_LIBS_USER = "{hostile_library_for_toml}",
    R_ENVIRON = "{hostile_environ_for_toml}",
    R_ENVIRON_USER = "{hostile_environ_for_toml}",
    R_PROFILE = "{hostile_profile_for_toml}",
    R_PROFILE_USER = "{hostile_profile_for_toml}"
}}
"#
        ),
    )
    .unwrap();

    let mut command = rv_cmd(&cache, &config);
    command.current_dir(project.path());
    command.arg("sync");
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let observed = fs::read_to_string(observed).unwrap();
    let values = probe_values(&observed);
    assert!(
        values["library"].contains("sandboxes"),
        "install-time .Library was not sandboxed: {observed}"
    );
    assert_eq!(values["site"], "", "unexpected .Library.site: {observed}");
    assert!(
        values["paths"].contains("rv/library") || values["paths"].contains("rv\\library"),
        "project library missing during install: {observed}"
    );
    assert!(
        values["paths"].contains("sandboxes"),
        "sandbox missing from install-time .libPaths(): {observed}"
    );
    assert!(
        !values["paths"].contains(&hostile_library.to_string_lossy().to_string()),
        "package environment leaked a host library into .libPaths(): {observed}"
    );
    for variable in ["r_libs", "r_libs_site", "r_libs_user"] {
        assert!(
            values[variable].contains("rv/library") || values[variable].contains("rv\\library"),
            "project library missing from {variable}: {observed}"
        );
        assert!(
            !values[variable].contains(&hostile_library.to_string_lossy().to_string()),
            "package environment overrode {variable}: {observed}"
        );
    }
    // The sandbox may be added by a controlled profile rather than encoded in
    // every R_LIBS* variable. What matters is the effective library search path
    // above and that package-supplied startup paths cannot replace rv's files.
    assert!(!values["r_profile"].is_empty(), "{observed}");
    assert!(!values["r_profile_user"].is_empty(), "{observed}");
    assert!(!values["r_environ"].is_empty(), "{observed}");
    assert!(!values["r_environ_user"].is_empty(), "{observed}");
    assert_ne!(
        values["r_profile"],
        hostile_profile.to_string_lossy(),
        "{observed}"
    );
    assert_ne!(
        values["r_profile_user"],
        hostile_profile.to_string_lossy(),
        "{observed}"
    );
    assert_ne!(
        values["r_environ"],
        hostile_environ.to_string_lossy(),
        "{observed}"
    );
    assert_ne!(
        values["r_environ_user"],
        hostile_environ.to_string_lossy(),
        "{observed}"
    );
    assert_eq!(values["environ_leak"], "", "{observed}");
    assert_eq!(values["profile_leak"], "", "{observed}");
}
