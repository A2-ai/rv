use std::path::{Path, PathBuf};
use std::process::Command;

use fs_err as fs;

use crate::consts::DESCRIPTION_FILENAME;
use crate::git::reference::{GitReference, Oid};
use crate::git::CommandExecutor;
use url::Url;

pub struct GitRepository {
    path: PathBuf,
    executor: Box<dyn CommandExecutor>,
}

impl GitRepository {
    pub(crate) fn rm_folder(&self) -> Result<(), std::io::Error> {
        if self.path.is_dir() {
            fs::remove_dir_all(&self.path)?;
        }
        Ok(())
    }

    pub fn open(
        path: impl AsRef<Path>,
        executor: impl CommandExecutor + 'static,
    ) -> Result<Self, std::io::Error> {
        // Only there to error if the folder is not a git repo
        let _ = executor.execute(Command::new("git").arg("rev-parse").current_dir(&path))?;

        Ok(Self {
            path: path.as_ref().into(),
            executor: Box::new(executor),
        })
    }

    /// This will init a git repository at the given path
    /// We do init instead of clone so we can fetch exactly what we need
    pub fn init(
        path: impl AsRef<Path>,
        executor: impl CommandExecutor + 'static,
    ) -> Result<Self, std::io::Error> {
        if !path.as_ref().is_dir() {
            fs::create_dir_all(&path)?;
        }
        log::debug!("Initializing git repository at {}", path.as_ref().display());
        let _ = executor.execute(Command::new("git").arg("init").current_dir(&path))?;

        Ok(Self {
            path: path.as_ref().into(),
            executor: Box::new(executor),
        })
    }

    pub fn fetch(&self, url: &Url, reference: &GitReference) -> Result<(), std::io::Error> {
        log::debug!("Fetching {} with reference {reference:?}", url);
        let refspecs = reference.as_refspecs();
        if refspecs.len() == 1 {
            fetch_with_cli(self, url, &refspecs[0], &*self.executor)
        } else {
            let mut errors: Vec<_> = refspecs
                .iter()
                .map_while(|refspec| {
                    match fetch_with_cli(self, url, refspec.as_str(), &*self.executor) {
                        Ok(_) => None,
                        Err(e) => {
                            log::debug!("Failed to fetch refspec `{refspec}`: {e}");
                            Some(e)
                        }
                    }
                })
                .collect();
            if errors.len() == refspecs.len() {
                Err(errors.pop().unwrap())
            } else {
                Ok(())
            }
        }
    }

    pub fn checkout(&self, oid: &Oid) -> Result<(), std::io::Error> {
        log::debug!("Doing git checkout {} in {}", oid.as_str(), self.path.display());
        self.executor
            .execute(
                Command::new("git")
                    .arg("checkout")
                    .arg(oid.as_str())
                    .current_dir(&self.path),
            )
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to checkout `{}`", oid.as_str()),
                )
            })?;
        Ok(())
    }

    /// Checks if we have that reference in the local repo.
    /// If we don't fetch it and try to checkout and read the DESCRIPTION file
    pub fn get_description_file_content(
        &self,
        url: &Url,
        reference: &GitReference,
        directory: Option<&PathBuf>,
    ) -> Result<String, std::io::Error> {
        log::debug!("Getting description file content of repo {url} at {reference:?}");
        self.fetch(url, reference)?;

        if let Some(oid) = self.ref_as_oid(reference.reference()) {
            self.checkout(&oid)?;

            let mut desc_path = self.path.clone();
            if let Some(d) = directory {
                desc_path = desc_path.join(d);
            }
            desc_path = desc_path.join(DESCRIPTION_FILENAME);
            if desc_path.exists() {
                return std::fs::read_to_string(desc_path);
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Not found",
        ))
    }

    /// Does a sparse checkout with just DESCRIPTION file checkout.
    pub fn sparse_checkout(
        &self,
        url: &Url,
        reference: &GitReference,
    ) -> Result<(), std::io::Error> {
        log::debug!("Doing a sparse checkout of {url} at {reference:?}");
        // 1. init sparse checkout
        self.executor.execute(
            Command::new("git")
                .arg("sparse-checkout")
                .arg("init")
                .current_dir(&self.path),
        )?;

        // 2. set the sparse checkout filter
        self.executor.execute(
            Command::new("git")
                .arg("sparse-checkout")
                .arg("set")
                // We only want a single file, not the top directory
                .arg("--no-cone")
                .arg("**/DESCRIPTION")
                .current_dir(&self.path),
        )?;

        // 3. perform the fetch
        self.fetch(url, reference)?;
        Ok(())
    }

    /// This only parses a branch/tag to a commit
    /// If the reference is a sha, it will just return itself but without checking whether
    /// it exists in the repo
    pub fn rev_parse(&self, reference: &str) -> Result<Oid, std::io::Error> {
        let output = self
            .executor
            .execute(
                Command::new("git")
                    .arg("rev-parse")
                    .arg(reference)
                    .current_dir(&self.path),
            )
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Reference {} not found", &reference),
                )
            })?;
        Ok(Oid::new(output))
    }

    pub fn ref_as_oid(&self, reference: &str) -> Option<Oid> {
        self.rev_parse(reference).ok()
    }
}

fn fetch_with_cli(
    repo: &GitRepository,
    url: &Url,
    refspec: &str,
    executor: &dyn CommandExecutor,
) -> Result<(), std::io::Error> {
    // https://github.com/astral-sh/uv/blob/main/crates/uv-git/src/git.rs#L572-L617
    let _ = executor
        .execute(
            Command::new("git")
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
                .env_remove("GIT_DIR"),
        )
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not fetch repository".to_string(),
            )
        })?;
    Ok(())
}
