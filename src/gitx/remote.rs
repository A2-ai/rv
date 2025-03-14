use std::path::Path;
use std::process::Command;

use url::Url;

use crate::consts::DESCRIPTION_FILENAME;
use crate::gitx::local::GitRepository;
use crate::gitx::reference::GitReference;

#[derive(Debug, Clone)]
pub struct GitRemote {
    url: Url,
}

impl GitRemote {
    pub fn new(url: &Url) -> Self {
        Self { url: url.clone() }
    }

    /// Fetch the minimum possible to only get the DESCRIPTION file.
    /// If the repository is already in the cache at `full_dest`, just checkout the reference and use that
    /// The sparse checkout will be done in a temp dir.
    /// This will return the body of the DESCRIPTION file if there was one
    /// TODO: handle directory
    pub fn sparse_checkout(
        &self,
        dest: impl AsRef<Path>,
        reference: &GitReference,
    ) -> Result<String, std::io::Error> {
        // If we have it locally try to only fetch what's needed
        if dest.as_ref().is_dir() {
            let local = GitRepository::open(dest.as_ref())?;
            // If we can't find the reference try to fetch it
            if local.ref_as_oid(reference.reference()).is_none() {
                local.fetch(&self.url, &reference)?;
            }

            let desc_path = dest.as_ref().join(DESCRIPTION_FILENAME);
            if desc_path.exists() {
                return std::fs::read_to_string(desc_path);
            }
        } else {
            let dir = tempfile::tempdir()?;

            let local = GitRepository::init(dir.path())?;

            // 1. init sparse checkout
            let _ = Command::new("git")
                .arg("sparse-checkout")
                .arg("init")
                .current_dir(dir.path())
                .output()?;

            // 2. set the sparse checkout filter
            let _ = Command::new("git")
                .arg("sparse-checkout")
                .arg("set")
                // We only want a single file, not the top directory
                .arg("--no-cone")
                .arg("DESCRIPTION")
                .current_dir(dir.path())
                .output()?;

            // 3. perform the fetch/checkout
            local.fetch(&self.url, reference)?;
            let oid = local.ref_as_oid(reference.reference());
            if let Some(oid) = oid {
                local.checkout(&oid)?;

                let desc_path = dir.path().join(DESCRIPTION_FILENAME);
                if desc_path.exists() {
                    return std::fs::read_to_string(desc_path);
                }
            }

            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Not found",
            ));
        }

        todo!("handle desc file not found")
    }

    pub fn checkout(
        &self,
        dest: impl AsRef<Path>,
        reference: &GitReference,
    ) -> Result<(), std::io::Error> {
        let repo = if dest.as_ref().is_dir() {
            GitRepository::open(dest.as_ref())?
        } else {
            GitRepository::init(dest.as_ref())?
        };

        // First we fetch if we can't find the reference locally
        let mut oid = repo.ref_as_oid(reference.reference());
        if oid.is_none() {
            repo.fetch(&self.url, reference)?;
            oid = repo.ref_as_oid(reference.reference());
        }
        if let Some(o) = oid {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sparse_checkout() {
        let remote = GitRemote::new(&Url::parse("https://github.com/A2-ai/scicalc").unwrap());
        let res = remote.sparse_checkout("nawak", &GitReference::Tag("v0.1.1"));
        println!("{:?}", res);
    }

    #[test]
    fn normal_checkout() {
        let remote = GitRemote::new(&Url::parse("https://github.com/A2-ai/scicalc").unwrap());
        let res = remote.checkout("testing", &GitReference::Tag("v0.1.1"));
        println!("{:?}", res);
        assert!(false);
    }
}
