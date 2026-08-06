//! Test-only helpers to run `git` without picking up the machine's git configuration.
//!
//! The tests build throwaway repositories, so the developer's git config changes what
//! those commands do, while CI stays green because nothing in the runner's own config
//! happens to touch them: `tag.gpgSign` alone turns the lightweight `git tag v1.0` into an
//! annotated tag, which then fails with `fatal: no tag message?`.
//!
//! Every git invocation in the test suite should go through [`run_git`] (for the commands
//! that set up a fixture) or [`IsolatedGitExecutor`] (for the commands rv itself runs).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::git::{CommandExecutor, GitExecutor};

const TEST_USER_NAME: &str = "Test User";
const TEST_USER_EMAIL: &str = "test@example.com";

/// A config file path that does not exist: git reads a missing config file as an empty
/// one, and unlike a real temporary file it leaves nothing behind to clean up.
fn empty_config_file() -> PathBuf {
    std::env::temp_dir().join("rv-git-test-isolation-nonexistent-gitconfig")
}

/// Makes a git command ignore the machine's configuration and environment.
pub(crate) fn isolate_git_env(command: &mut Command) -> &mut Command {
    let empty_config = empty_config_file();

    command.env_clear();

    // Kept so git can find the helpers it shells out to, and so the tests run the same git
    // the developer's shell does: on Unix a cleared PATH does not make the spawn fail, it
    // falls back to `confstr(_CS_PATH)`, which on macOS quietly picks Apple's
    // `/usr/bin/git` over the Homebrew one. On Windows PATH is what resolves `git` at all.
    // It cannot affect git's configuration. Left unset rather than set to an empty string
    // when the parent has no PATH, since an empty PATH turns that fallback off.
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }

    command
        // Taking `HOME`/`XDG_CONFIG_HOME` away already hides the global config, but only
        // for as long as nothing puts `HOME` back, so pin it. GIT_CONFIG_GLOBAL replaces
        // both `~/.gitconfig` and `$XDG_CONFIG_HOME/git/config`, so those need no handling
        // of their own.
        .env("GIT_CONFIG_GLOBAL", &empty_config)
        // The system config is found by compiled-in path, so `env_clear` does not stop git
        // from reading it: without this the tests see whatever is in `/etc/gitconfig` or,
        // under Homebrew, `$(brew --prefix)/etc/gitconfig`.
        .env("GIT_CONFIG_SYSTEM", &empty_config)
        // GIT_CONFIG_SYSTEM only exists since git 2.32
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_ATTR_NOSYSTEM", "1")
        // Without a user config there is no user.name/user.email to commit or tag with
        .env("GIT_AUTHOR_NAME", TEST_USER_NAME)
        .env("GIT_AUTHOR_EMAIL", TEST_USER_EMAIL)
        .env("GIT_COMMITTER_NAME", TEST_USER_NAME)
        .env("GIT_COMMITTER_EMAIL", TEST_USER_EMAIL)
        // No credential prompts: a test that blocks on one never finishes
        .env("GIT_TERMINAL_PROMPT", "0")
}

/// The [`GitExecutor`] rv uses in production, with the test isolation applied to every
/// command before it runs.
#[derive(Debug, Clone)]
pub(crate) struct IsolatedGitExecutor;

impl CommandExecutor for IsolatedGitExecutor {
    fn execute(&self, command: &mut Command) -> Result<String, std::io::Error> {
        GitExecutor.execute(isolate_git_env(command))
    }
}

/// Runs a git command in `dir` to set up a test fixture, panicking if it fails.
///
/// Note that this is not a test itself: the assert is there so a failing fixture command
/// points at what went wrong instead of at the test that used its result.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the isolation itself: without [`isolate_git_env`] the same command lists the
    /// `global` and `system` entries of whatever machine it runs on. That includes CI, where
    /// the macOS image installs git with Homebrew and leaves `safe.directory`, a set of
    /// `advice.*` keys and a system-wide LFS filter behind, so this is not a local-only check.
    #[test]
    fn no_config_from_outside_the_repository_is_visible() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = Command::new("git");
        command
            .arg("config")
            .arg("--list")
            .arg("--show-scope")
            .current_dir(dir.path());

        let output = isolate_git_env(&mut command).output().unwrap();
        let listed = String::from_utf8_lossy(&output.stdout);
        let leaked: Vec<_> = listed
            .lines()
            .filter(|line| line.starts_with("global") || line.starts_with("system"))
            .collect();

        assert!(
            leaked.is_empty(),
            "the machine's git config is visible to the tests:\n{}",
            leaked.join("\n")
        );
    }
}
