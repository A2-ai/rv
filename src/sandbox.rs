//! In some environments, users can install packages directly in R RHOME, leaking packages
//! into every R sessions so scripts can work locally but break elsewhere because of dependencies
//! not listed in rproject.toml or rv.lock.
//! To avoid that, we create sandboxes in the cache where we link the base + recommended packages only
//! from the given R RHOME. Any user packages will NOT be linked into the sandbox.
//! The sandbox in the cache is keyed like this:
//! `caches/rv/sandboxes/{R install hash}/{R-major.minor}/{arch}/{distro}/{hash of base+rec packages}`
//! so the sandbox can be shared across projects using the same R.
//!
//! The sandbox is an opt-in behaviour at either the config or environment variable level and is
//! used in the activate script.
//! It is not currently used in rv R INSTALL of packages.
use std::path::{Path, PathBuf};

use fs_err as fs;
use sha2::{Digest, Sha256};

use crate::Cache;
use crate::consts::{BASE_PACKAGES, RECOMMENDED_PACKAGES};
use crate::package::{Package, parse_description_file};
use crate::sync::LinkMode;

// R ships translations beside its base packages, but it is not returned by
// installed.packages(priority = "base") and has no Priority field.
const R_TRANSLATIONS_PACKAGE: &str = "translations";

#[derive(Debug, thiserror::Error)]
pub enum SandboxErrorKind {
    #[error("IO error: {error} ({path})")]
    File {
        error: std::io::Error,
        path: PathBuf,
    },
    #[error("base package `{name}` is missing or broken in the R library ({library})")]
    MissingBasePackage {
        name: &'static str,
        library: PathBuf,
    },
    #[error(transparent)]
    LibraryNotFound(#[from] crate::r_cmd::LibraryError),
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
#[non_exhaustive]
pub struct SandboxError {
    pub source: SandboxErrorKind,
}

impl SandboxError {
    /// For use in `map_err` on fs operations, capturing the path involved
    fn file(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |error| Self {
            source: SandboxErrorKind::File { error, path },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SandboxPackages {
    packages: Vec<(PathBuf, Package)>,
}

impl SandboxPackages {
    pub fn sha(&self) -> String {
        let mut hasher = Sha256::new();
        for (_, pkg) in &self.packages {
            hasher.update(format!("{}-{}\n", pkg.name, pkg.version.original));
        }
        let result = hex::encode(hasher.finalize());
        result[..10].to_string()
    }

    /// We start with symlinks but it can be an issue on Windows.
    /// If that doesn't work, then we try hardlinks and copy as last fallback
    pub fn materialize_to(&self, path: &Path) -> Result<(), SandboxError> {
        // Clear whatever is there first so we have a fresh sandbox
        if path.is_dir() {
            fs::remove_dir_all(path).map_err(SandboxError::file(path))?;
        }
        fs::create_dir_all(path).map_err(SandboxError::file(path))?;

        let mut mode = LinkMode::Symlink;
        for (lib_path, pkg) in &self.packages {
            let dest = path.join(&pkg.name);

            while let Err(error) = mode.link_package_dir(lib_path, &dest) {
                // A failed attempt can leave a partial result behind
                if let Ok(meta) = fs::symlink_metadata(&dest)
                    && meta.is_dir()
                {
                    fs::remove_dir_all(&dest).map_err(SandboxError::file(&dest))?;
                }

                let fallback = match mode {
                    LinkMode::Symlink => LinkMode::Hardlink,
                    LinkMode::Hardlink => LinkMode::Copy,
                    _ => {
                        return Err(SandboxError::file(dest)(std::io::Error::other(error)));
                    }
                };
                log::warn!(
                    "Could not {} {} into the sandbox: {error}. Falling back to {}.",
                    mode.name(),
                    pkg.name,
                    fallback.name()
                );
                mode = fallback;
            }
        }

        Ok(())
    }
}

pub fn get_packages_to_copy(library: &Path) -> Result<SandboxPackages, SandboxError> {
    let mut pkgs = Vec::new();

    for entry in fs::read_dir(library).map_err(SandboxError::file(library))? {
        let entry = entry.map_err(SandboxError::file(library))?;

        let path = entry.path();
        let description_path = path.join("DESCRIPTION");
        if !path.is_dir() || !description_path.exists() {
            continue;
        }
        let name = entry.file_name().as_os_str().to_string_lossy().to_string();
        if !BASE_PACKAGES.contains(&name.as_str())
            && !RECOMMENDED_PACKAGES.contains(&name.as_str())
            && name != R_TRANSLATIONS_PACKAGE
        {
            continue;
        }
        let content =
            fs::read_to_string(&description_path).map_err(SandboxError::file(description_path))?;
        let Some(package) = parse_description_file(&content) else {
            continue;
        };

        pkgs.push((entry.path(), package));
    }

    pkgs.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    Ok(SandboxPackages { packages: pkgs })
}

pub fn ensure_sandbox_exists(library: &Path, cache: &Cache) -> Result<PathBuf, SandboxError> {
    let content = get_packages_to_copy(library)?;
    let content_sha = content.sha();
    let (local, global) = cache.get_sandbox_paths(library);
    if let Some(g) = global {
        let path = g.join(&content_sha);
        if path.is_dir() {
            return Ok(path);
        }
    }
    let sandbox_path = local.join(&content_sha);
    if sandbox_path.is_dir() {
        return Ok(sandbox_path);
    }

    fs::create_dir_all(&local).map_err(SandboxError::file(&local))?;
    let tmp = tempfile::tempdir_in(&local).map_err(SandboxError::file(&local))?;
    content.materialize_to(tmp.path())?;

    for name in BASE_PACKAGES {
        if !tmp.path().join(name).join("DESCRIPTION").is_file() {
            return Err(SandboxError {
                source: SandboxErrorKind::MissingBasePackage {
                    name,
                    library: library.to_path_buf(),
                },
            });
        }
    }

    match fs::rename(tmp.path(), &sandbox_path) {
        Ok(()) => Ok(sandbox_path),
        Err(error) => {
            if sandbox_path.is_dir() {
                Ok(sandbox_path)
            } else {
                Err(SandboxError::file(sandbox_path)(error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_package(library: &Path, name: &str, version: &str) {
        let package = library.join(name);
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("DESCRIPTION"),
            format!("Package: {name}\nVersion: {version}\n"),
        )
        .unwrap();
    }

    #[test]
    fn materialize_filters_out_non_builtin_packages() {
        let library = tempfile::tempdir().unwrap();
        add_package(library.path(), "base", "4.5.0");
        add_package(library.path(), "MASS", "7.3-65");
        add_package(library.path(), R_TRANSLATIONS_PACKAGE, "4.5.0");
        add_package(library.path(), "leaked", "1.0.0");

        let packages = get_packages_to_copy(library.path()).unwrap();
        let out = tempfile::tempdir().unwrap();
        packages.materialize_to(out.path()).unwrap();

        assert!(out.path().join("base").join("DESCRIPTION").is_file());
        assert!(out.path().join("MASS").join("DESCRIPTION").is_file());
        assert!(
            out.path()
                .join(R_TRANSLATIONS_PACKAGE)
                .join("DESCRIPTION")
                .is_file()
        );
        assert!(!out.path().join("leaked").exists());
    }
}
