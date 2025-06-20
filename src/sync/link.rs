//! How packages are linked from the cache to each project library
//! Taken from uv: clone (CoW) on MacOS and hard links on Mac/Linux by default
//! Maybe with optional symlink support for cross disk linking

use fs_err as fs;
use fs_err::DirEntry;
use reflink_copy as reflink;
use std::env;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::fs::copy_folder;

const LINK_ENV_NAME: &str = "RV_LINK_MODE";

#[derive(thiserror::Error, Debug)]
pub enum LinkError {
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
    /// Use symlinks for all elements
    Symlink,
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
    pub fn new() -> Self {
        // First try to find out if the mode is set in the env
        let from_env = if let Ok(val) = env::var(LINK_ENV_NAME) {
            match val.to_lowercase().as_str() {
                "copy" => Some(Self::Copy),
                "clone" => Some(Self::Clone),
                "hardlink" => Some(Self::Hardlink),
                "symlink" => Some(Self::Symlink),
                _ => None,
            }
        } else {
            None
        };

        from_env.unwrap_or_default()
    }

    /// Try to symlink if possible and fallback to copy on Windows
    /// Windows requires admin rights for symlink so it might not work oob
    pub fn symlink_if_possible() -> Self {
        if cfg!(target_os = "windows") {
            Self::Copy
        } else {
            Self::Symlink
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Clone => "clone",
            Self::Symlink => "symlink",
            Self::Hardlink => "hardlink",
        }
    }

    pub fn link_files(
        &self,
        package_name: &str,
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<(), LinkError> {
        // If it's already exists for some reasons (eg failed halfway before), delete it first
        let pkg_in_lib = destination.as_ref().join(package_name);
        if pkg_in_lib.is_dir() {
            fs::remove_dir_all(&pkg_in_lib)?;
        }

        let res = match self {
            LinkMode::Copy => {
                copy_folder(source.as_ref(), destination.as_ref()).map_err(Into::into)
            }
            LinkMode::Clone => clone_package(source.as_ref(), destination.as_ref()),
            LinkMode::Hardlink => hardlink_package(source.as_ref(), destination.as_ref()),
            LinkMode::Symlink => symlink_package(source.as_ref(), destination.as_ref()),
        };

        if let Err(e) = res {
            if *self == LinkMode::Copy {
                return Err(e);
            }
            // Cleanup a bit in case it failed halfway through
            if pkg_in_lib.is_dir() {
                fs::remove_dir_all(&pkg_in_lib)?
            }
            log::warn!(
                "Failed to {} files: {e}. Falling back to copying files.",
                self.name()
            );
            copy_folder(source.as_ref(), destination.as_ref())?;
        }

        Ok(())
    }
}

/// macOS can copy directories recursively but Windows/Linux need to clone file by file
fn clone_recursive(source: &Path, library: &Path, entry: &DirEntry) -> Result<(), LinkError> {
    let from = entry.path();
    let to = library.join(from.strip_prefix(source).unwrap());

    if (cfg!(windows) || cfg!(target_os = "linux")) && from.is_dir() {
        fs::create_dir_all(&to)?;
        for entry in fs::read_dir(from)? {
            clone_recursive(source, library, &entry?)?;
        }
        return Ok(());
    }

    reflink::reflink(&from, &to).map_err(|err| LinkError::Reflink { from, to, err })?;
    Ok(())
}

// Taken from uv
fn clone_package(source: &Path, library: &Path) -> Result<(), LinkError> {
    for entry in fs::read_dir(source)? {
        clone_recursive(source, library, &entry?)?;
    }

    Ok(())
}

// Same as copy but hardlinking instead
fn hardlink_package(source: &Path, library: &Path) -> Result<(), LinkError> {
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let path = entry.path();

        let relative = path.strip_prefix(source).expect("walkdir starts with root");
        let out_path = library.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        fs::hard_link(path, out_path)?;
    }

    Ok(())
}

fn symlink_package(source: &Path, library: &Path) -> Result<(), LinkError> {
    log::debug!(
        "Linking package from {} to {} using symlinks",
        source.display(),
        library.display()
    );
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let path = entry.path();

        let relative = path.strip_prefix(source).expect("walkdir starts with root");
        let out_path = library.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        // if symlink not created we want to log which specific entry was problematic before
        // bubbling up the error
        match create_symlink(path, &out_path) {
            Ok(_) => continue,
            Err(e) => {
                log::error!(
                    "Failed to create symlink from {} to {}, error: {}",
                    path.display(),
                    out_path.display(),
                    e
                );
                return Err(LinkError::Io(e));
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
fn create_symlink(original: impl AsRef<Path>, link: impl AsRef<Path>) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn create_symlink(original: impl AsRef<Path>, link: impl AsRef<Path>) -> std::io::Result<()> {
    if original.as_ref().is_dir() {
        std::os::windows::fs::symlink_dir(original, link)
    } else {
        std::os::windows::fs::symlink_file(original, link)
    }
}
