use std::path::{Path, PathBuf};

use fs_err as fs;
use sha2::{Digest, Sha256};

use crate::Cache;
use crate::consts::{BASE_PACKAGES, RECOMMENDED_PACKAGES};
use crate::package::{Package, parse_description_file};
use crate::sync::create_symlink;

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

    pub fn symlink_to(&self, path: &Path) -> Result<(), SandboxError> {
        // Clear whatever is there first so we have a fresh sandbox
        if path.is_dir() {
            fs::remove_dir_all(path).map_err(SandboxError::file(path))?;
        }
        fs::create_dir_all(path).map_err(SandboxError::file(path))?;

        for (lib_path, pkg) in &self.packages {
            let link = path.join(&pkg.name);
            create_symlink(lib_path, &link).map_err(SandboxError::file(link))?;
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
        if !BASE_PACKAGES.contains(&name.as_str()) && !RECOMMENDED_PACKAGES.contains(&name.as_str())
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
    content.symlink_to(tmp.path())?;

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
