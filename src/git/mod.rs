use std::path::Path;
use std::process::Command;

mod local;
mod reference;
mod remote;
pub(crate) mod url;

pub trait CommandExecutor {
    fn execute(&self, command: &mut Command) -> Result<String, std::io::Error>;
}

pub use local::GitRepository;
pub use reference::GitReference;
pub use remote::GitRemote;

const SYMREF_PREFIX: &str = "ref: refs/heads/";

/// Resolves the default branch of a remote git repository by URL.
pub fn resolve_default_branch_for_url(
    executor: &dyn CommandExecutor,
    url: &str,
) -> Result<String, std::io::Error> {
    ls_remote_symref_head(executor, url, None)
}

/// Resolves the default branch of an already-initialized repository's `origin` remote.
pub fn resolve_default_branch_in_repo(
    executor: &dyn CommandExecutor,
    repo_path: &Path,
) -> Result<String, std::io::Error> {
    ls_remote_symref_head(executor, "origin", Some(repo_path))
}

fn ls_remote_symref_head(
    executor: &dyn CommandExecutor,
    target: &str,
    cwd: Option<&Path>,
) -> Result<String, std::io::Error> {
    let mut command = Command::new("git");
    command
        .arg("ls-remote")
        .arg("--symref")
        .arg(target)
        .arg("HEAD");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let output = executor.execute(&mut command)?;
    // The output is something like:
    // > git ls-remote --symref origin HEAD
    // ref: refs/heads/main    HEAD
    // 8823d1d2b5f4d80ed77f781e30df2eaa4c84fe9e        HEAD
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix(SYMREF_PREFIX)
            && let Some(name) = rest.split_whitespace().next()
        {
            return Ok(name.trim().to_string());
        }
    }

    Err(std::io::Error::other(format!(
        "Could not determine default branch for `{target}` from `git ls-remote --symref` output:\n{output}"
    )))
}

#[derive(Debug, Clone)]
pub struct GitExecutor;

impl CommandExecutor for GitExecutor {
    fn execute(&self, command: &mut Command) -> Result<String, std::io::Error> {
        let res = command.output()?;
        if res.status.success() {
            Ok(String::from_utf8_lossy(&res.stdout).trim().to_string())
        } else {
            Err(std::io::Error::other(String::from_utf8_lossy(&res.stderr)))
        }
    }
}
