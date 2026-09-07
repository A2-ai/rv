use std::collections::HashMap;
use std::fs;

use assert_cmd::cargo;
use tempfile::TempDir;

/// Reports what R ended up with, as `key\tvalue` lines.
const PROBE: &str = r#"cat(
    paste("library", .Library, sep = "\t"),
    paste("paths", paste(.libPaths(), collapse = "|"), sep = "\t"),
    paste("sys_lib_on_path", if (normalizePath(file.path(R.home(), "library"), mustWork = FALSE) %in% normalizePath(.libPaths(), mustWork = FALSE)) "yes" else "no", sep = "\t"),
    paste("profile", Sys.getenv("RV_HOST_PROFILE"), sep = "\t"),
    paste("environ", Sys.getenv("RV_HOST_ENVIRON"), sep = "\t"),
    paste("r_libs_site", Sys.getenv("R_LIBS_SITE"), sep = "\t"),
    paste("repos", paste(getOption("repos"), collapse = "|"), sep = "\t"),
    sep = "\n"
)"#;

/// A project with startup files that try to take `.libPaths()` over and leave a trace behind:
/// `.Rprofile` calls `.libPaths()` directly, `.Renviron` goes through `R_LIBS_USER`. Both are
/// what an unrelated rv project the user happens to be sitting in would effectively do.
fn create_project(sandbox: bool) -> (TempDir, TempDir, std::path::PathBuf) {
    let project = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let shadow = project.path().join("shadow-library");
    fs::create_dir(&shadow).unwrap();

    fs::write(
        project.path().join(".Rprofile"),
        format!(
            "Sys.setenv(RV_HOST_PROFILE = 'profile-loaded')\n.libPaths({:?}, include.site = FALSE)\n",
            shadow.to_str().unwrap()
        ),
    )
    .unwrap();
    fs::write(
        project.path().join(".Renviron"),
        format!(
            "RV_HOST_ENVIRON=environ-loaded\nR_LIBS_USER={}\n",
            shadow.to_str().unwrap()
        ),
    )
    .unwrap();

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
        .env("PATH", path_with_rv())
        .args(["--config-file", config.to_str().unwrap()]);
    command
}

/// A generated activate script calls whatever `rv` is on the PATH, so the binary under test has
/// to be there or it warns and does nothing, and the activated tests would pass on a no-op.
fn path_with_rv() -> std::ffi::OsString {
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_rv"));
    let mut dirs = vec![binary.parent().unwrap().to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(dirs).unwrap()
}

fn probe(cache: &TempDir, config: &std::path::Path, extra: &[&str]) -> HashMap<String, String> {
    let mut command = rv_cmd(cache, config);
    command.current_dir(config.parent().unwrap()).arg("run");
    command.args(extra);
    command.args(["--no-sync", "-e", PROBE]);
    parse_probe(command)
}

/// Runs the command and collects the `key\tvalue` lines it printed.
fn parse_probe(mut command: assert_cmd::Command) -> HashMap<String, String> {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.split_once('\t')
                .map(|(key, value)| (key.to_string(), value.to_string()))
        })
        .collect()
}

/// Builds the sandbox and returns where it landed, so the tests can compare `.Library` to it
/// rather than to a substring that would also match some unrelated path.
fn sandbox_path(cache: &TempDir, config: &std::path::Path) -> String {
    let mut command = rv_cmd(cache, config);
    command.args(["info", "--sandbox"]);
    let output = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = stdout
        .trim()
        .strip_prefix("sandbox:")
        .unwrap_or_else(|| panic!("unexpected `rv info --sandbox` output: {stdout}"))
        .trim()
        .to_string();
    assert!(!path.is_empty(), "the sandbox was not built: {stdout}");
    path
}

#[test]
fn run_loads_host_startup_files_by_default() {
    let (_project, cache, config) = create_project(false);
    let observed = probe(&cache, &config, &[]);

    // Both files ran, and between them they took the library over
    assert_eq!(observed["profile"], "profile-loaded", "{observed:?}");
    assert_eq!(observed["environ"], "environ-loaded", "{observed:?}");
    assert!(observed["paths"].contains("shadow-library"), "{observed:?}");
}

#[test]
fn run_isolated_ignores_host_startup_files() {
    let (_project, cache, config) = create_project(false);
    let observed = probe(&cache, &config, &["--isolated"]);

    assert_eq!(observed["profile"], "", "{observed:?}");
    assert_eq!(observed["environ"], "", "{observed:?}");
    assert!(
        !observed["paths"].contains("shadow-library"),
        "{observed:?}"
    );
    // No sandbox was asked for, so `.Library` is still the system one
    assert_eq!(observed["sys_lib_on_path"], "yes", "{observed:?}");
}

/// The sandbox is not conditional on `--isolated`: it applies on its own, and startup files
/// keep loading so an interactive session and `rv run` do not diverge.
#[test]
fn run_uses_the_sandbox_while_still_loading_host_startup_files() {
    let (_project, cache, config) = create_project(true);
    let sandbox = sandbox_path(&cache, &config);
    let observed = probe(&cache, &config, &[]);

    assert_eq!(observed["library"], sandbox, "{observed:?}");
    assert_eq!(observed["sys_lib_on_path"], "no", "{observed:?}");
    assert_eq!(observed["profile"], "profile-loaded", "{observed:?}");
    assert_eq!(observed["environ"], "environ-loaded", "{observed:?}");
}

#[test]
fn run_isolated_with_a_sandbox_drops_both_the_system_library_and_the_startup_files() {
    let (_project, cache, config) = create_project(true);
    let sandbox = sandbox_path(&cache, &config);
    let observed = probe(&cache, &config, &["--isolated"]);

    assert_eq!(observed["library"], sandbox, "{observed:?}");
    assert_eq!(observed["sys_lib_on_path"], "no", "{observed:?}");
    assert_eq!(observed["profile"], "", "{observed:?}");
    assert_eq!(observed["environ"], "", "{observed:?}");
    assert!(
        !observed["paths"].contains("shadow-library"),
        "{observed:?}"
    );
    // The sandbox is last, so nothing got appended past it
    assert!(observed["paths"].ends_with(&sandbox), "{observed:?}");
}

#[test]
fn the_activate_script_sets_repos_but_leaves_the_library_to_rv_run() {
    const REPO: &str = "https://packagemanager.posit.co/cran/2025-05-12";
    let project = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();
    let config = project.path().join("rproject.toml");
    fs::write(
        &config,
        format!(
            r#"[project]
name = "test-run-activated"
r_version = "4.5"
sandbox = true
repositories = [
    {{alias = "posit", url = "{REPO}/"}}
]
dependencies = []
"#
        ),
    )
    .unwrap();

    let mut activate = rv_cmd(&cache, &config);
    let output = activate
        .current_dir(project.path())
        .arg("activate")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.path().join(".Rprofile").is_file());

    let sandbox = sandbox_path(&cache, &config);
    let observed = probe(&cache, &config, &[]);

    // Only the activate script sets these, so their presence is how we know it ran at all
    assert!(observed["repos"].contains(REPO), "{observed:?}");
    // The sandbox in effect is the one `rv run` established
    assert_eq!(observed["library"], sandbox, "{observed:?}");
    // ...and the script left the library paths as `rv run` set them. When it re-derived them it
    // wrote `R_LIBS_SITE` as the project library *and* the sandbox; `rv run` writes only the former
    assert!(!observed["r_libs_site"].contains(&sandbox), "{observed:?}");
}

/// `R_LIBS_USER`/`R_LIBS_SITE` outlive the working directory they were computed against: the
/// script can `setwd` and anything it then spawns - a `parallel` worker, `callr`, another
/// Rscript - resolves them afresh. A relative library would silently drop out at that point.
#[test]
fn the_library_survives_a_setwd_in_anything_the_script_spawns() {
    let (project, cache, _config) = create_project(false);
    let child = project.path().join("child.R");
    fs::write(
        &child,
        r#"cat("child", paste(.libPaths(), collapse = "|"), sep = "\t")
cat("\n")
"#,
    )
    .unwrap();
    let script = project.path().join("script.R");
    fs::write(
        &script,
        format!(
            "setwd(tempdir())\nwriteLines(system2(\"Rscript\", {:?}, stdout = TRUE))\n",
            child.to_str().unwrap()
        ),
    )
    .unwrap();

    // Deliberately *not* `rv_cmd`: it passes `--config-file` as an absolute path, which makes
    // the library path absolute too and hides the bug. The default is a relative
    // `rproject.toml` in the working directory, and that is what yields a relative library.
    // `--isolated` so the fixture's own hostile `.Rprofile` is not what we end up measuring.
    let mut command = cargo::cargo_bin_cmd!();
    command
        .env("RV_CACHE_DIR", cache.path())
        .env("PATH", path_with_rv())
        .current_dir(project.path())
        .args(["run", "--isolated", "--no-sync"])
        .arg(&script);
    let observed = parse_probe(command);

    assert!(
        observed["child"].contains("rv/library") || observed["child"].contains("rv\\library"),
        "the spawned R lost the project library after setwd: {observed:?}"
    );
}
