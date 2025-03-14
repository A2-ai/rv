use std::path::{Path, PathBuf};
use std::process::Command;

use fs_err as fs;

use crate::gitx::reference::{GitReference, Oid};
use url::Url;

#[derive(Debug, Clone)]
pub struct GitRepository {
    path: PathBuf,
}

impl GitRepository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        // This will error if the folder is not a git repository
        Command::new("git")
            .arg("rev-parse")
            .current_dir(&path)
            .output()?;

        Ok(Self {
            path: path.as_ref().into(),
        })
    }

    /// This will init a git repository at the given path
    /// We do init instead of clone so we can fetch exactly what we need
    pub fn init(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        if !path.as_ref().is_dir() {
            fs::create_dir_all(&path)?;
        }
        Command::new("git")
            .arg("init")
            .current_dir(&path)
            .output()?;

        Ok(Self {
            path: path.as_ref().into(),
        })
    }

    pub fn fetch(&self, url: &Url, reference: &GitReference) -> Result<(), std::io::Error> {
        let refspecs = reference.as_refspecs();
        if refspecs.len() == 1 {
            fetch_with_cli(&self, url, &refspecs[0])
        } else {
            let mut errors: Vec<_> = refspecs
                .iter()
                .map_while(
                    |refspec| match fetch_with_cli(&self, url, refspec.as_str()) {
                        Ok(_) => None,
                        Err(e) => {
                            log::debug!("Failed to fetch refspec `{refspec}`: {e}");
                            Some(e)
                        }
                    },
                )
                .collect();
            if errors.len() == refspecs.len() {
                Err(errors.pop().unwrap())
            } else {
                Ok(())
            }
        }
    }

    pub fn checkout(&self, oid: &Oid) -> Result<(), std::io::Error> {
        let output = Command::new("git")
            .arg("checkout")
            .arg(oid.as_str())
            .current_dir(&self.path)
            .output()?;

        if output.status.success() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to checkout `{}`", oid.as_str()),
            ))
        }
    }

    pub fn rev_parse(&self, reference: &str) -> Result<Oid, std::io::Error> {
        let output = Command::new("git")
            .arg("rev-parse")
            .arg(reference)
            .current_dir(&self.path)
            .output()?;

        if output.status.success() {
            let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(Oid::new(sha));
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Reference {} not found", &reference),
        ))
    }

    pub fn ref_as_oid(&self, reference: &str) -> Option<Oid> {
        self.rev_parse(reference).ok()
    }
}

fn fetch_with_cli(repo: &GitRepository, url: &Url, refspec: &str) -> Result<(), std::io::Error> {
    println!("Fetching refspec `{refspec}`");
    // https://github.com/astral-sh/uv/blob/main/crates/uv-git/src/git.rs#L572-L617
    let mut cmd = Command::new("git")
        .arg("fetch")
        .arg("--tags")
        .arg("--force")
        .arg("--update-head-ok")
        .arg(url.as_str())
        .arg(refspec)
        .current_dir(&repo.path)
        // Disable interactive prompts
        .env("GIT_TERMINAL_PROMPT", "0")
        // From Cargo
        // If rv is run by git (for example, the `exec` command in `git
        // rebase`), the GIT_DIR is set by git and will point to the wrong
        // location (this takes precedence over the cwd). Make sure this is
        // unset so git will look at cwd for the repo.
        .env_remove("GIT_DIR")
        .output()?;

    if cmd.status.success() {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not fetch repository".to_string(),
        ))
    }
}
