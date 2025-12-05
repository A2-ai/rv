use std::path::{Path, PathBuf};
use std::process::Command;

use fs_err as fs;

use crate::consts::{DESCRIPTION_FILENAME, SUBMODULE_UPDATE_DISABLE_ENV_VAR_NAME};
use crate::git::CommandExecutor;
use crate::git::reference::{GitReference, Oid};
use crate::utils::is_env_var_truthy;

const HEAD_LINE_START: &str = "HEAD branch: ";

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
        url: &str,
        executor: impl CommandExecutor + 'static,
    ) -> Result<Self, std::io::Error> {
        log::debug!("Opening git repository at {}", path.as_ref().display());
        // Only there to error if the folder is not a git repo
        if executor
            .execute(Command::new("git").arg("rev-parse").current_dir(&path))
            .is_err()
        {
            fs::remove_dir_all(&path)?;
            return Self::init(path, url, executor);
        }

        Ok(Self {
            path: path.as_ref().into(),
            executor: Box::new(executor),
        })
    }

    /// This will init a git repository at the given path
    /// We do init instead of clone so we can fetch exactly what we need
    pub fn init(
        path: impl AsRef<Path>,
        url: &str,
        executor: impl CommandExecutor + 'static,
    ) -> Result<Self, std::io::Error> {
        log::debug!("Initializing git repository at {}", path.as_ref().display());
        if !path.as_ref().is_dir() {
            fs::create_dir_all(&path)?;
        }
        let _ = executor.execute(Command::new("git").arg("init").current_dir(&path))?;
        let _ = executor.execute(
            Command::new("git")
                .arg("remote")
                .arg("add")
                .arg("origin")
                .arg(url)
                .current_dir(&path),
        )?;

        Ok(Self {
            path: path.as_ref().into(),
            executor: Box::new(executor),
        })
    }

    pub fn fetch(&self, url: &str, reference: &GitReference) -> Result<(), std::io::Error> {
        // Before fetching, checks whether the oid exists locally
        // We only do that for commits since tag/branches could have changed remotely
        // so finding a reference locally is not meaningful
        if let GitReference::Commit(c) = reference
            && let Some(oid) = self.ref_as_oid(c)
            && self
                .executor
                .execute(
                    Command::new("git")
                        .arg("cat-file")
                        .arg("-e")
                        .arg(oid.as_str())
                        .current_dir(&self.path),
                )
                .is_ok()
        {
            log::debug!("No need to fetch {url}, reference {reference:?} is already found locally");
            return Ok(());
        }

        log::debug!("Fetching {url} with reference {reference:?}");
        let refspecs = reference.as_refspecs();
        if refspecs.len() == 1 {
            fetch_with_cli(self, url, &refspecs[0], &*self.executor)?;
        } else {
            let mut errors: Vec<_> = refspecs
                .iter()
                .map_while(|refspec| {
                    match fetch_with_cli(self, url, refspec.as_str(), &*self.executor) {
                        Ok(_) => None,
                        Err(e) => {
                            println!("Failed to fetch {}", refspec);
                            log::debug!("Failed to fetch refspec `{refspec}`: {e}");
                            Some(e)
                        }
                    }
                })
                .collect();
            if errors.len() == refspecs.len() {
                return Err(errors.pop().unwrap());
            }
        }

        // if we have a branch fetch won't create it locally so we need to checkout
        // otherwise there's nothing to rev-parse
        if self.rev_parse(reference.reference()).is_err() {
            match reference {
                GitReference::Branch(branch) => {
                    self.checkout_branch(branch)?;
                }
                GitReference::Unknown(ref_name) => {
                    if *ref_name == "HEAD" {
                        self.checkout_head()?;
                    } else {
                        // Check if this unknown reference corresponds to a remote branch
                        // After fetching with Unknown, branches are available as origin/branch_name
                        if self.rev_parse(&format!("origin/{}", ref_name)).is_ok() {
                            self.checkout_branch(ref_name)?;
                            // or it could be a tag?
                        } else if let Ok(oid) = self.rev_parse(&format!("origin/tags/{}", ref_name))
                        {
                            self.checkout(&oid)?;
                        }
                    }
                }
                _ => (),
            }
        }

        self.force_update_local_reference(reference)?;

        Ok(())
    }

    pub fn checkout(&self, oid: &Oid) -> Result<(), std::io::Error> {
        if let Ok(head_oid) = self.rev_parse("HEAD") {
            // If HEAD is already our reference, no need to checkout
            if &head_oid == oid {
                log::debug!(
                    "No need to checkout {}, it's already checked out",
                    oid.as_str()
                );
                return Ok(());
            }
        };

        log::debug!(
            "Doing git checkout {} in {}",
            oid.as_str(),
            self.path.display()
        );
        self.executor
            .execute(
                Command::new("git")
                    .arg("checkout")
                    .arg(oid.as_str())
                    .current_dir(&self.path),
            )
            .map_err(|_| std::io::Error::other(format!("Failed to checkout `{}`", oid.as_str())))?;

        self.update_submodules()?;
        Ok(())
    }

    pub fn checkout_branch(&self, branch_name: &str) -> Result<(), std::io::Error> {
        log::debug!(
            "Doing git checkout -B {branch_name} in {}",
            self.path.display()
        );
        self.executor
            .execute(
                Command::new("git")
                    .arg("checkout")
                    .arg("-B")
                    .arg(branch_name)
                    .arg(format!("origin/{branch_name}"))
                    .current_dir(&self.path),
            )
            .map_err(|_| {
                std::io::Error::other(format!("Failed to checkout branch `{branch_name}`"))
            })?;

        self.update_submodules()?;
        Ok(())
    }

    /// If we don't know what we have, we will fetch the HEAD branch
    fn checkout_head(&self) -> Result<(), std::io::Error> {
        let output = self.executor.execute(
            Command::new("git")
                .arg("remote")
                .arg("show")
                .arg("origin")
                .current_dir(&self.path),
        )?;
        let mut branch_name = String::new();

        for l in output.lines() {
            if l.trim().starts_with(HEAD_LINE_START) {
                branch_name = l.replace(HEAD_LINE_START, "").trim().to_string();
            }
        }

        if branch_name.is_empty() {
            return Err(std::io::Error::other(format!(
                "No HEAD branch found, output of CLI was:\n{output}\n"
            )));
        }

        self.checkout_branch(&branch_name)
    }

    /// Checks if we have that reference in the local repo.
    pub fn get_description_file_content(
        &self,
        url: &str,
        reference: &GitReference,
        directory: Option<&PathBuf>,
    ) -> Result<String, std::io::Error> {
        log::debug!(
            "Getting description file content of repo {url} at {reference:?} in {}",
            self.path.display()
        );
        if let Some(oid) = self.ref_as_oid(reference.reference()) {
            self.checkout(&oid)?;

            let mut desc_path = self.path.clone();
            if let Some(d) = directory {
                desc_path = desc_path.join(d);
            }
            desc_path = desc_path.join(DESCRIPTION_FILENAME);
            if desc_path.exists() {
                return std::fs::read_to_string(desc_path);
            } else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "DESCRIPTION file not found",
                ));
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
        url: &str,
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

    pub fn disable_sparse_checkout(&self) -> Result<(), std::io::Error> {
        log::debug!("Disabling sparse checkout in {}", self.path.display());
        self.executor.execute(
            Command::new("git")
                .arg("sparse-checkout")
                .arg("disable")
                .current_dir(&self.path),
        )?;

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
            .map_err(|_| std::io::Error::other(format!("Reference {} not found", &reference)))?;
        Ok(Oid::new(output))
    }

    pub fn ref_as_oid(&self, reference: &str) -> Option<Oid> {
        self.rev_parse(reference).ok()
    }

    fn update_submodules(&self) -> Result<(), std::io::Error> {
        if is_env_var_truthy(SUBMODULE_UPDATE_DISABLE_ENV_VAR_NAME) {
            log::debug!("Skipping update submodule as env var is truthy");
            return Ok(());
        }

        self.executor
            .execute(
                Command::new("git")
                    .arg("submodule")
                    .arg("update")
                    .arg("--init")
                    .arg("--recursive")
                    .current_dir(&self.path),
            )
            .map_err(|_| std::io::Error::other("Failed to update submodules".to_string()))?;
        Ok(())
    }

    /// Force update local references to match remote after fetching
    fn force_update_local_reference(&self, reference: &GitReference) -> Result<(), std::io::Error> {
        match reference {
            GitReference::Branch(branch) => {
                // Check if we're currently on this branch
                let current_branch = self.executor.execute(
                    Command::new("git")
                        .arg("branch")
                        .arg("--show-current")
                        .current_dir(&self.path),
                );

                let is_current_branch =
                    current_branch.map(|b| b.trim() == *branch).unwrap_or(false);

                if is_current_branch {
                    // If we're on this branch, we need to reset it instead of force updating
                    self.executor.execute(
                        Command::new("git")
                            .arg("reset")
                            .arg("--hard")
                            .arg(format!("origin/{}", branch))
                            .current_dir(&self.path),
                    )?;
                } else {
                    // Force update local branch to match remote
                    self.executor.execute(
                        Command::new("git")
                            .arg("branch")
                            .arg("-f")
                            .arg(branch)
                            .arg(format!("origin/{}", branch))
                            .current_dir(&self.path),
                    )?;
                }
            }
            GitReference::Tag(tag) => {
                // Force update local tag to match remote
                self.executor.execute(
                    Command::new("git")
                        .arg("tag")
                        .arg("-f")
                        .arg(tag)
                        .arg(format!("origin/tags/{}", tag))
                        .current_dir(&self.path),
                )?;
            }
            _ => {} // Commits don't need updating
        }
        Ok(())
    }
}

fn fetch_with_cli(
    repo: &GitRepository,
    url: &str,
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
                .arg(url)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::GitExecutor;
    use std::process::Command;

    fn run_git(args: &[&str], dir: &Path) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn setup_test_repo() -> (tempfile::TempDir, String) {
        let temp_dir = tempfile::tempdir().unwrap();
        let remote_path = temp_dir.path().join("remote");
        let work_path = temp_dir.path().join("work");

        // Setup bare remote repo
        std::fs::create_dir_all(&remote_path).unwrap();
        run_git(&["init", "--bare"], &remote_path);

        // Clone and setup working repo
        std::fs::create_dir_all(&work_path).unwrap();
        run_git(&["clone", remote_path.to_str().unwrap(), "."], &work_path);
        run_git(&["config", "user.email", "test@example.com"], &work_path);
        run_git(&["config", "user.name", "Test User"], &work_path);

        // Create initial commit
        std::fs::write(work_path.join("file.txt"), "initial content").unwrap();
        run_git(&["add", "."], &work_path);
        run_git(&["commit", "-m", "initial"], &work_path);

        let branch_name = "main";
        run_git(&["checkout", "-b", branch_name], &work_path);
        run_git(&["push", "origin", branch_name], &work_path);

        (temp_dir, branch_name.to_string())
    }

    #[test]
    fn test_branch_update_after_checkout() {
        let (temp_dir, branch_name) = setup_test_repo();
        let remote_path = temp_dir.path().join("remote");
        let cache_path = temp_dir.path().join("cache");
        let work_path = temp_dir.path().join("work");

        let repo =
            GitRepository::init(&cache_path, remote_path.to_str().unwrap(), GitExecutor).unwrap();

        // First fetch
        repo.fetch(
            remote_path.to_str().unwrap(),
            &GitReference::Branch(&branch_name),
        )
        .unwrap();
        let initial_oid = repo.ref_as_oid(&branch_name).unwrap();

        // Update remote
        std::fs::write(work_path.join("file.txt"), "updated content").unwrap();
        run_git(&["add", "."], &work_path);
        run_git(&["commit", "-m", "updated"], &work_path);
        run_git(&["push", "origin", &branch_name], &work_path);

        // Second fetch should get updated commit
        repo.fetch(
            remote_path.to_str().unwrap(),
            &GitReference::Branch(&branch_name),
        )
        .unwrap();
        let updated_oid = repo.ref_as_oid(&branch_name).unwrap();

        assert_ne!(initial_oid.as_str(), updated_oid.as_str());

        // Verify checkout gives us updated content
        repo.checkout(&updated_oid).unwrap();
        let content = std::fs::read_to_string(cache_path.join("file.txt")).unwrap();
        assert_eq!(content, "updated content");
    }

    #[test]
    fn test_tag_update_after_checkout() {
        let (temp_dir, branch_name) = setup_test_repo();
        let remote_path = temp_dir.path().join("remote");
        let cache_path = temp_dir.path().join("cache");
        let work_path = temp_dir.path().join("work");

        // Create and push initial tag
        run_git(&["tag", "v1.0"], &work_path);
        run_git(&["push", "origin", "v1.0"], &work_path);

        let repo =
            GitRepository::init(&cache_path, remote_path.to_str().unwrap(), GitExecutor).unwrap();

        // First fetch
        repo.fetch(remote_path.to_str().unwrap(), &GitReference::Tag("v1.0"))
            .unwrap();
        let initial_oid = repo.ref_as_oid("v1.0").unwrap();

        // Update remote and move tag
        std::fs::write(work_path.join("file.txt"), "updated content").unwrap();
        run_git(&["add", "."], &work_path);
        run_git(&["commit", "-m", "updated"], &work_path);
        run_git(&["tag", "-f", "v1.0"], &work_path);
        run_git(&["push", "origin", &branch_name], &work_path);
        run_git(&["push", "origin", "v1.0", "--force"], &work_path);

        // Second fetch should get updated commit
        repo.fetch(remote_path.to_str().unwrap(), &GitReference::Tag("v1.0"))
            .unwrap();
        let updated_oid = repo.ref_as_oid("v1.0").unwrap();

        assert_ne!(initial_oid.as_str(), updated_oid.as_str());

        // Verify checkout gives us updated content
        repo.checkout(&updated_oid).unwrap();
        let content = std::fs::read_to_string(cache_path.join("file.txt")).unwrap();
        assert_eq!(content, "updated content");
    }
}
