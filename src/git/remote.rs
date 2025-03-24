use std::path::{Path, PathBuf};

use crate::git::{CommandExecutor, GitReference, GitRepository};

#[derive(Debug, Clone)]
pub struct GitRemote {
    url: String,
    directory: Option<PathBuf>,
}

impl GitRemote {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            directory: None,
        }
    }

    pub fn set_directory(&mut self, directory: &str) {
        self.directory = Some(PathBuf::from(directory));
    }

    /// Fetch the minimum possible to only get the DESCRIPTION file.
    /// If the repository is already in the cache at `full_dest`, just checkout the reference and use that
    /// This will return the body of the DESCRIPTION file if there was one as well as the oid.
    /// Only used during resolution
    pub fn sparse_checkout_for_description(
        &self,
        dest: impl AsRef<Path>,
        reference: &GitReference,
        executor: impl CommandExecutor + Clone + 'static,
    ) -> Result<(String, String), std::io::Error> {
        // If we have it locally try to only fetch what's needed
        if dest.as_ref().is_dir() {
            let local = GitRepository::open(dest.as_ref(), &self.url, executor)?;
            local.fetch(&self.url, reference)?;
            let content = local.get_description_file_content(
                &self.url,
                reference,
                self.directory.as_ref(),
            )?;
            let oid = local.ref_as_oid(reference.reference()).unwrap();
            Ok((oid.as_str().to_string(), content))
        } else {
            let local = GitRepository::init(dest.as_ref(), &self.url, executor)?;
            local.fetch(&self.url, reference)?;
            match local.sparse_checkout(&self.url, reference) {
                Ok(_) => (),
                Err(e) => {
                    // Ensure we delete the folder so another resolution will not find it
                    local.rm_folder()?;
                    return Err(e);
                }
            }

            let content = local.get_description_file_content(
                &self.url,
                reference,
                self.directory.as_ref(),
            )?;
            let oid = local.ref_as_oid(reference.reference()).unwrap();
            Ok((oid.as_str().to_string(), content))
        }
    }

    pub fn checkout(
        &self,
        dest: impl AsRef<Path>,
        reference: &GitReference,
        executor: impl CommandExecutor + Clone + 'static,
    ) -> Result<(), std::io::Error> {
        let repo = if dest.as_ref().is_dir() {
            GitRepository::open(dest.as_ref(), &self.url, executor)?
        } else {
            GitRepository::init(dest.as_ref(), &self.url, executor)?
        };

        repo.disable_sparse_checkout()?;
        repo.fetch(&self.url, reference)?;
        if let Some(o) = repo.ref_as_oid(reference.reference()) {
            repo.checkout(&o)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to find reference {:?}", reference),
            ));
        }

        Ok(())
    }
}
