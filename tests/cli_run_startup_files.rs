use std::collections::HashMap;
use std::fs;

use assert_cmd::cargo;
use tempfile::TempDir;

/// Reports what R ended up with, as `key\tvalue` lines.
///
/// Lists are joined with `+`, not `|`: Windows `Rscript.exe` relaunches `R.exe` through
/// `cmd.exe`, which reads a `|` in the `-e` expression as a pipe and cuts the command in half.
/// Keep this expression free of anything else cmd claims - `&`, `<`, `>`, `^`.
const PROBE: &str = r#"cat(
    paste("library", .Library, sep = "\t"),
    paste("paths", paste(.libPaths(), collapse = "+"), sep = "\t"),
    paste("sys_lib_on_path", if (normalizePath(file.path(R.home(), "library"), mustWork = FALSE) %in% normalizePath(.libPaths(), mustWork = FALSE)) "yes" else "no", sep = "\t"),
    paste("profile", Sys.getenv("RV_HOST_PROFILE"), sep = "\t"),
    paste("environ", Sys.getenv("RV_HOST_ENVIRON"), sep = "\t"),
    paste("site", Sys.getenv("RV_SITE_PROFILE"), sep = "\t"),
    paste("r_libs_site", Sys.getenv("R_LIBS_SITE"), sep = "\t"),
    paste("repos", paste(getOption("repos"), collapse = "+"), sep = "\t"),
    sep = "\n"
)"#;

/// `-e` reaches Rscript as a single argument, and on Windows an embedded newline cuts the
/// expression short ("unexpected end of input"), so it goes over as one line there. Collapsing
/// newlines is only safe because [PROBE] is a single call with no comments in it.
fn one_line(code: &str) -> String {
    code.replace('\n', " ")
}

/// The project's repository, and the one a self-contained script declares instead. Different
/// values are the whole point: they are how a test tells whose configuration won.
const PROJECT_REPO: &str = "https://packagemanager.posit.co/cran/2025-05-12";
const SCRIPT_REPO: &str = "https://packagemanager.posit.co/cran/2023-06-01";

/// An activated project whose startup files each leave a marker behind, so a test can tell
/// whether they ran at all.
///
/// Deliberately *not* part of the fixture: a `.Renviron` setting `R_LIBS_USER`. R reads it before
/// any profile and it would take the library over, for `rv run` and for anything the script
/// spawns. That is knowingly the project's own doing and not defended against.
fn create_project(sandbox: bool) -> (TempDir, TempDir, std::path::PathBuf) {
    let project = TempDir::new().unwrap();
    let cache = TempDir::new().unwrap();

    fs::write(
        project.path().join(".Rprofile"),
        "Sys.setenv(RV_HOST_PROFILE = 'profile-loaded')\n",
    )
    .unwrap();
    fs::write(
        project.path().join(".Renviron"),
        "RV_HOST_ENVIRON=environ-loaded\n",
    )
    .unwrap();

    let config = project.path().join("rproject.toml");
    fs::write(
        &config,
        format!(
            r#"library = "project-library"

sandbox = {sandbox}
[project]
name = "test-run-startup-files"
r_version = "4.5"
repositories = [
    {{alias = "posit", url = "{PROJECT_REPO}/"}}
]
dependencies = []
"#
        ),
    )
    .unwrap();

    // Prepends the `source("rv/scripts/activate.R")` call to the `.Rprofile` written above
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

fn probe(cache: &TempDir, config: &std::path::Path) -> HashMap<String, String> {
    let mut command = rv_cmd(cache, config);
    command
        .current_dir(config.parent().unwrap())
        .args(["run", "--no-sync", "-e"])
        .arg(one_line(PROBE));
    parse_probe(command)
}

/// Same probe, but through a script carrying its own config, which is the case where the project
/// activate script has to stay out of it entirely.
fn probe_self_contained(
    cache: &TempDir,
    config: &std::path::Path,
    sandbox: bool,
) -> HashMap<String, String> {
    let project = config.parent().unwrap();
    let script = project.join("script.R");
    fs::write(
        &script,
        format!(
            r#"# /// rv
# sandbox = {sandbox}
# [project]
# r_version = "4.5"
# repositories = [
#     {{ alias = "script-repo", url = "{SCRIPT_REPO}/" }}
# ]
# dependencies = []
# ///
{PROBE}
"#
        ),
    )
    .unwrap();

    let mut command = rv_cmd(cache, config);
    command
        .current_dir(project)
        .args(["run", "--no-sync"])
        .arg(&script);
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

fn normalize_path(path: &str) -> String {
    let path = std::path::Path::new(path);
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let resolved = resolved.to_string_lossy().replace('\\', "/");
    let resolved = resolved.strip_prefix("//?/").unwrap_or(&resolved);
    if cfg!(windows) {
        resolved.to_lowercase()
    } else {
        resolved.to_string()
    }
}

/// The `+`-joined `.libPaths()` the probe reports, each entry normalized.
fn normalized_paths(observed: &HashMap<String, String>) -> Vec<String> {
    observed["paths"].split('+').map(normalize_path).collect()
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

/// Projects put more than rv activation in their startup files, so `rv run` leaves them alone.
#[test]
fn run_loads_host_startup_files() {
    let (_project, cache, config) = create_project(false);
    let observed = probe(&cache, &config);

    assert_eq!(observed["profile"], "profile-loaded", "{observed:?}");
    assert_eq!(observed["environ"], "environ-loaded", "{observed:?}");
    assert!(
        observed["paths"].contains("project-library"),
        "{observed:?}"
    );
    // The repositories come from the profile `rv run` writes, out of the config it resolved -
    // the activate script, which would have answered from the working directory, stood down
    assert!(observed["repos"].contains(PROJECT_REPO), "{observed:?}");
}

/// The startup files of the project a self-contained script happens to be run from still load -
/// only the rv activation in them is asked to stand down - and so nothing of that project's
/// configuration reaches the script: not its library, not its repositories.
#[test]
fn run_of_a_self_contained_script_keeps_host_startup_files_but_takes_nothing_from_the_project() {
    let (_project, cache, config) = create_project(false);
    let observed = probe_self_contained(&cache, &config, false);

    assert_eq!(observed["profile"], "profile-loaded", "{observed:?}");
    assert_eq!(observed["environ"], "environ-loaded", "{observed:?}");
    assert!(observed["repos"].contains(SCRIPT_REPO), "{observed:?}");
    assert!(!observed["repos"].contains(PROJECT_REPO), "{observed:?}");
    assert!(
        !observed["paths"].contains("project-library"),
        "{observed:?}"
    );
}

/// A site profile is not the user's to lose: a sandboxed run needs `R_PROFILE` for the `.Library`
/// rebinding, so it sources whatever that displaces first.
#[test]
fn a_sandboxed_run_still_loads_the_site_profile_it_displaces() {
    let (project, cache, config) = create_project(true);
    let sandbox = sandbox_path(&cache, &config);
    let site = project.path().join("Rprofile.site");
    fs::write(&site, "Sys.setenv(RV_SITE_PROFILE = 'site-loaded')\n").unwrap();

    let mut command = rv_cmd(&cache, &config);
    command
        .current_dir(project.path())
        .env("R_PROFILE", &site)
        .args(["run", "--no-sync", "-e"])
        .arg(one_line(PROBE));
    let observed = parse_probe(command);

    assert_eq!(observed["site"], "site-loaded", "{observed:?}");
    // ...and it is sourced first, so the sandbox still took effect
    assert_eq!(
        normalize_path(&observed["library"]),
        normalize_path(&sandbox),
        "{observed:?}"
    );
}

/// The sandbox is not tied to dropping the startup files: it applies on its own, and they keep
/// loading so an interactive session and `rv run` do not diverge.
#[test]
fn run_uses_the_sandbox_while_still_loading_host_startup_files() {
    let (_project, cache, config) = create_project(true);
    let sandbox = sandbox_path(&cache, &config);
    let observed = probe(&cache, &config);

    assert_eq!(
        normalize_path(&observed["library"]),
        normalize_path(&sandbox),
        "{observed:?}"
    );
    assert_eq!(observed["sys_lib_on_path"], "no", "{observed:?}");
    assert_eq!(observed["profile"], "profile-loaded", "{observed:?}");
    assert_eq!(observed["environ"], "environ-loaded", "{observed:?}");
    // `rv run` writes only the project library there; the activate script would have appended
    // the sandbox to it, which is how we know it contributed nothing
    let sandbox = normalize_path(&sandbox);
    let r_libs_site = std::env::split_paths(&observed["r_libs_site"])
        .map(|p| normalize_path(&p.to_string_lossy()))
        .collect::<Vec<_>>();
    assert!(!r_libs_site.contains(&sandbox), "{observed:?}");
}

#[test]
fn run_of_a_self_contained_script_with_a_sandbox_drops_the_system_library_too() {
    let (_project, cache, config) = create_project(true);
    // The sandbox is keyed on the R install, not the project, so the one the script gets is the
    // one this builds
    let sandbox = sandbox_path(&cache, &config);
    let observed = probe_self_contained(&cache, &config, true);

    let sandbox = normalize_path(&sandbox);
    assert_eq!(
        normalize_path(&observed["library"]),
        sandbox,
        "{observed:?}"
    );
    assert_eq!(observed["sys_lib_on_path"], "no", "{observed:?}");
    assert!(
        !observed["paths"].contains("project-library"),
        "{observed:?}"
    );
    // The sandbox is last, so nothing got appended past it
    assert_eq!(
        normalized_paths(&observed).last(),
        Some(&sandbox),
        "{observed:?}"
    );
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
        r#"cat("child", paste(.libPaths(), collapse = "+"), sep = "\t")
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
    let mut command = cargo::cargo_bin_cmd!();
    command
        .env("RV_CACHE_DIR", cache.path())
        .env("PATH", path_with_rv())
        .current_dir(project.path())
        .args(["run", "--no-sync"])
        .arg(&script);
    let observed = parse_probe(command);

    assert!(
        observed["child"].contains("project-library"),
        "the spawned R lost the project library after setwd: {observed:?}"
    );
}
