//! Test-only helpers to run `git` without picking up the machine's git configuration.
//!
//! The tests build throwaway repositories, so the developer's git config changes what
//! those commands do, while CI stays green because nothing in the runner's own config
//! happens to touch them: `tag.gpgSign` alone turns the lightweight `git tag v1.0` into an
//! annotated tag, which then fails with `fatal: no tag message?`.
//!
//! Every git invocation in *this crate's* unit tests should go through [`run_git`] (for
//! the commands that set up a fixture) or [`IsolatedGitExecutor`] (for the commands rv
//! itself runs). The integration tests in `tests/` cannot: this module is `#[cfg(test)]`,
//! so it does not exist for them, and they drive the rv binary, which uses the production
//! [`GitExecutor`] and therefore the machine's config. `tests/cli_global_cache.rs` syncs a
//! real git dependency that way.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::git::{CommandExecutor, GitExecutor};

const TEST_USER_NAME: &str = "Test User";
const TEST_USER_EMAIL: &str = "test@example.com";

/// An empty config file, in a directory this process owns, kept for the lifetime of the
/// test binary and removed when it exits.
///
/// It has to be a path no one else can write to, not merely a path that happens not to
/// exist: git reads whatever `GIT_CONFIG_GLOBAL` points at, so a well-known name under the
/// shared temp directory (`/tmp` on Linux) is one `touch` by any other user away from
/// feeding the tests an attacker's config, and `core.pager`, `alias.*` and `include.path`
/// all run commands. `tempfile` creates the directory with owner-only permissions.
fn empty_config_file() -> PathBuf {
    static EMPTY_CONFIG_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();

    let dir = EMPTY_CONFIG_DIR.get_or_init(|| {
        let dir = tempfile::tempdir().expect("failed to create the git test isolation dir");
        std::fs::write(dir.path().join("gitconfig"), "")
            .expect("failed to create the empty git config");
        dir
    });

    dir.path().join("gitconfig")
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

    // Git for Windows is an msys2 binary, and the Windows APIs it links need a few
    // variables to find the system itself: without `SystemRoot` the winsock and crypto
    // DLLs fail to initialise, and without `TEMP`/`TMP` git cannot write the temporary
    // files it uses for merges and locks. None of them can point git at a configuration
    // file. `USERPROFILE`, `HOMEDRIVE` and `HOMEPATH` are deliberately left cleared: they
    // are how git locates the user's global config on Windows.
    #[cfg(windows)]
    for name in [
        "SystemRoot",
        "SystemDrive",
        "WINDIR",
        "TEMP",
        "TMP",
        "COMSPEC",
        "PATHEXT",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
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
///
/// The isolation lands after the calling code has built its command, so the `env_clear` in
/// [`isolate_git_env`] drops whatever environment that code set up: `fetch_with_cli`'s
/// `GIT_TERMINAL_PROMPT` and `GIT_DIR` handling is replaced by the equivalent here rather
/// than exercised. Anything the production commands grow beyond those needs mirroring in
/// [`isolate_git_env`], or the tests quietly stop running what rv runs.
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

        isolate_git_env(&mut command);
        for (name, value) in extra_env {
            command.env(name, value);
        }

        let output = command.output().unwrap();
        // Without this the test passes on an empty stdout, which is exactly what a git too
        // old for `--show-scope` (< 2.26) produces before failing.
        assert!(
            output.status.success(),
            "git config --list --show-scope failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn assert_nothing_outside_the_repository_leaked(listed: &str, context: &str) {
        let leaked: Vec<_> = listed
            .lines()
            .filter(|line| line.starts_with("global") || line.starts_with("system"))
            .collect();

        assert!(
            leaked.is_empty(),
            "git config from outside the repository is visible to the tests ({context}):\n{}",
            leaked.join("\n")
        );
    }

    /// Guards the isolation itself. On a machine with a user config, the same command run
    /// without [`isolate_git_env`] lists `global` and often `system` entries; CI has no
    /// user config, so this one only ever fails locally.
    #[test]
    fn the_machines_config_is_not_visible() {
        assert_nothing_outside_the_repository_leaked(
            &isolated_config_list(&[]),
            "the machine's own config",
        );
    }

    /// The companion that fails everywhere, CI included: it brings its own config instead
    /// of relying on the developer having one. `env_clear` alone would not be enough here,
    /// since these are set again afterwards — this is what pinning `GIT_CONFIG_GLOBAL`
    /// buys, and it has to keep winning over both places git looks for a global config.
    #[test]
    fn a_restored_home_does_not_bring_a_global_config_back() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join(".gitconfig"),
            "[user]\n\tname = leaked-from-home\n",
        )
        .unwrap();

        let listed = isolated_config_list(&[("HOME", home.path())]);
        assert_nothing_outside_the_repository_leaked(&listed, "HOME put back");
        assert!(
            !listed.contains("leaked-from-home"),
            "$HOME/.gitconfig is visible to the tests:\n{listed}"
        );

        let xdg = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(xdg.path().join("git")).unwrap();
        std::fs::write(
            xdg.path().join("git").join("config"),
            "[user]\n\tname = leaked-from-xdg\n",
        )
        .unwrap();

        let listed = isolated_config_list(&[("XDG_CONFIG_HOME", xdg.path())]);
        assert_nothing_outside_the_repository_leaked(&listed, "XDG_CONFIG_HOME put back");
        assert!(
            !listed.contains("leaked-from-xdg"),
            "$XDG_CONFIG_HOME/git/config is visible to the tests:\n{listed}"
        );
    }
}
