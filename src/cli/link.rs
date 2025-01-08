//! How packages are linked from the cache to each project library
//! Taken from uv: clone (CoW) on MacOS and hard links on Mac/Linux by default
//! Maybe with optional symlink support for cross disk linking

use std::path::{Path, PathBuf};

use fs_err as fs;
use fs_err::DirEntry;
use reflink_copy as reflink;
use walkdir::WalkDir;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Failed to walk the directory")]
    WalkDir(#[from] walkdir::Error),
    #[error("Failed to reflink {from:?} to {to:?}")]
    Reflink {
        from: PathBuf,
        to: PathBuf,
        #[source]
        err: std::io::Error,
    },
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LinkMode {
    /// Copy all files. The slowest option
    Copy,
    /// Copy files with CoW
    Clone,
    /// Use hardlinks for all elements
    Hardlink,
    // Symlink,
}

impl Default for LinkMode {
    fn default() -> Self {
        if cfg!(target_os = "macos") {
            Self::Clone
        } else {
            Self::Hardlink
        }
    }
}

impl LinkMode {
    pub fn link_files(
        &self,
        source: impl AsRef<Path>,
        library: impl AsRef<Path>,
    ) -> Result<(), Error> {
        // TODO: make sure the output directory does not exist
        // TODO: fallback to copy if clone/hardlink fails
        match self {
            LinkMode::Copy => copy_package(source.as_ref(), library.as_ref()),
            LinkMode::Clone => clone_package(source.as_ref(), library.as_ref()),
            LinkMode::Hardlink => hardlink_package(source.as_ref(), library.as_ref()),
        }
    }
}

/// Copy the whole content of a package to the given library, file by file.
fn copy_package(source: &Path, library: &Path) -> Result<(), Error> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let path = entry.path();

        let relative = path
            .strip_prefix(&source)
            .expect("walkdir starts with root");
        let out_path = library.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        fs::copy(path, out_path)?;
    }

    Ok(())
}

/// macOS can copy directories recursively but Windows/Linux need to clone file by file
fn clone_recursive(source: &Path, library: &Path, entry: &DirEntry) -> Result<(), Error> {
    let from = entry.path();
    let to = library.join(from.strip_prefix(source).unwrap());
    log::debug!("Cloning {from:?} to {to:?}");

    if (cfg!(windows) || cfg!(target_os = "linux")) && from.is_dir() {
        fs::create_dir_all(&to)?;
        for entry in fs::read_dir(from)? {
            clone_recursive(source, library, &entry?)?;
        }
        return Ok(());
    }

    reflink::reflink(&from, &to).map_err(|err| Error::Reflink { from, to, err })?;
    Ok(())
}

// Taken from uv
fn clone_package(source: &Path, library: &Path) -> Result<(), Error> {
    for entry in fs::read_dir(source)? {
        clone_recursive(source, library, &entry?)?;
    }

    Ok(())
}

// Same as copy but hardlinking instead
fn hardlink_package(source: &Path, library: &Path) -> Result<(), Error> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let path = entry.path();

        let relative = path
            .strip_prefix(&source)
            .expect("walkdir starts with root");
        let out_path = library.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        fs::hard_link(path, out_path)?;
    }

    Ok(())
}
