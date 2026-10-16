//! Test-only helpers to run `git` without picking up the machine's git configuration.
//!
//! The tests build throwaway repositories, so whatever config the machine carries changes what
//! those commands do: `tag.gpgSign` alone turns the lightweight `git tag v1.0` into an annotated
//! tag, which then fails with `fatal: no tag message?`.

use std::path::Path;
use std::process::Command;

use crate::git::{CommandExecutor, GitExecutor};

const TEST_USER_NAME: &str = "Test User";
const TEST_USER_EMAIL: &str = "test@example.com";

/// Makes a git command ignore the machine's configuration and environment.
pub(crate) fn isolate_git_env(command: &mut Command) -> &mut Command {
    // No `HOME`/`XDG_CONFIG_HOME` means no global config, attributes or ignore file, and no
    // `GIT_*` from the surrounding shell either: `GIT_DIR` would point the fixtures at another
    // repository outright.
    command.env_clear();

    // Not for the spawn itself, which finds git either way: a cleared PATH falls back to a system
    // default, and on the macOS CI runner that quietly swaps the Homebrew git for the older Apple
    // one inside Xcode. PATH cannot affect git's configuration.
    //
    // NOTE: On Windows most environmental variables used by an msys2 binary like Git for Windows
    // (SystemDrive, WINDIR, TEMP, TMP, COMSPEC, PATHEXT) are not needed for any of the fixtures
    // (bare init, clone, add, commit, push, tag, fetch). However, a test that fetches over the
    // network would need SystemRoot back on Windows.
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }

    command
        // The system config is found by compiled-in path, so `env_clear` does not hide it
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        // Without a global config there is no user.name/user.email to commit or tag with
        .env("GIT_AUTHOR_NAME", TEST_USER_NAME)
        .env("GIT_AUTHOR_EMAIL", TEST_USER_EMAIL)
        .env("GIT_COMMITTER_NAME", TEST_USER_NAME)
        .env("GIT_COMMITTER_EMAIL", TEST_USER_EMAIL)
        // No credential prompts: a test that blocks on one never finishes
        .env("GIT_TERMINAL_PROMPT", "0")
}

/// The [`GitExecutor`] rv uses in production, with the isolation applied to every command.
///
/// It lands after the calling code has built its command, so the `env_clear` drops whatever
/// environment that code set up. Anything the production commands grow beyond what
/// [`isolate_git_env`] puts back needs mirroring there.
#[derive(Debug, Clone)]
pub(crate) struct IsolatedGitExecutor;

impl CommandExecutor for IsolatedGitExecutor {
    fn execute(&self, command: &mut Command) -> Result<String, std::io::Error> {
        GitExecutor.execute(isolate_git_env(command))
    }
}

/// Runs an isolated git command in `dir` to set up a test fixture, panicking if it fails so that
/// no test passes on the empty output of a command that did not run.
pub(crate) fn run_git(args: &[&str], dir: &Path) {
    let mut command = Command::new("git");
    command.args(args).current_dir(dir);

    let output = isolate_git_env(&mut command).output().unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}
