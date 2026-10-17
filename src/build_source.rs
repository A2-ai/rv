//! Prepare ordinary local packages without walking their ignored rv state.
//!
//! R CMD build copies the entire source (dereferencing symlinks) before applying
//! .Rbuildignore. Recognize only literal, anchored exclusions of rv or its
//! library here. Keep rv/scripts and leave every other exclusion to R, since
//! ignored files can be build-script inputs.

use std::path::{Path, PathBuf};

use fs_err as fs;
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::consts::{LIBRARY_ROOT_DIR_NAME, RV_DIR_NAME};
use crate::fs::preserve_mtime;

fn excludes_library(contents: &[u8]) -> bool {
    contents.split(|byte| *byte == b'\n').any(|line| {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        line.eq_ignore_ascii_case(b"^rv$") || line.eq_ignore_ascii_case(b"^rv/library$")
    })
}

/// The guard owns the scratch tree until R has finished reading it.
pub(crate) struct BuildSource {
    _scratch: TempDir,
    pub path: PathBuf,
}

pub(crate) fn prepare(source: &Path) -> Result<Option<BuildSource>, std::io::Error> {
    if !source
        .join(RV_DIR_NAME)
        .join(LIBRARY_ROOT_DIR_NAME)
        .is_dir()
    {
        return Ok(None);
    }
    let ignore = match fs::read(source.join(".Rbuildignore")) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !excludes_library(&ignore) {
        return Ok(None);
    }

    let scratch = tempfile::tempdir()?;
    let path = scratch.path().join(source.file_name().unwrap_or_default());
    copy_source(source, &path)?;
    log::debug!(
        "Staged {} in {}, pruning rv/library as excluded by .Rbuildignore",
        source.display(),
        path.display()
    );
    Ok(Some(BuildSource {
        _scratch: scratch,
        path,
    }))
}

fn copy_source(source: &Path, destination: &Path) -> Result<(), std::io::Error> {
    let mut directories = Vec::new();
    let library = source.join(RV_DIR_NAME).join(LIBRARY_ROOT_DIR_NAME);
    // Like R's cp -LR, materialize retained symlinks. Preserving relative links
    // while relocating the source would silently change their targets.
    for entry in WalkDir::new(source)
        .follow_links(true)
        .into_iter()
        .filter_entry(|entry| entry.path() != library.as_path())
    {
        let entry = entry?;
        let target = destination.join(entry.path().strip_prefix(source).unwrap());
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            fs::create_dir_all(&target)?;
            directories.push((target, metadata));
        } else {
            fs::copy(entry.path(), &target)?;
            preserve_mtime(&target, &metadata);
        }
    }
    // Restore directory metadata after populating them, including read-only
    // directories and the mtimes changed by creating their children.
    for (path, metadata) in directories.into_iter().rev() {
        fs::set_permissions(&path, metadata.permissions())?;
        preserve_mtime(&path, &metadata);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs::Permissions;

    #[test]
    fn recognizes_only_the_literal_rule() {
        for contents in [
            b"^rv$".as_slice(),
            b"^RV$\r\n",
            b"^other$\n^rv$\n",
            b"^rv/library$\n",
        ] {
            assert!(excludes_library(contents));
        }
        for contents in [
            b"^rv/".as_slice(),
            b"rv",
            b" ^rv$",
            b"^rv$ ",
            b"^(rv)$",
            b"# ^rv$",
        ] {
            assert!(!excludes_library(contents));
        }
    }

    #[test]
    fn leaves_unrecognized_or_missing_exclusions_to_r() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir_all(source.path().join("rv/library")).unwrap();
        assert!(prepare(source.path()).unwrap().is_none());
        fs::write(source.path().join(".Rbuildignore"), "^rv/\n").unwrap();
        assert!(prepare(source.path()).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn prunes_before_traversal_and_materializes_retained_relative_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("pkg");
        fs::create_dir_all(source.join("rv/library")).unwrap();
        fs::create_dir_all(source.join("rv/scripts")).unwrap();
        fs::create_dir_all(source.join("inst/rv")).unwrap();
        fs::create_dir_all(source.join("tools")).unwrap();
        fs::write(source.join(".Rbuildignore"), "^rv$\n^tools$\n").unwrap();
        fs::write(source.join("tools/input"), "build input").unwrap();
        fs::write(source.join("inst/rv/keep"), "nested rv").unwrap();
        fs::write(source.join("rv/scripts/activate.R"), "build helper").unwrap();
        fs::write(root.path().join("shared"), "outside source").unwrap();
        fs::set_permissions(root.path().join("shared"), Permissions::from_mode(0o755)).unwrap();
        symlink("../shared", source.join("configure")).unwrap();
        // A walk entering the ignored library would fail on this cycle.
        symlink(".", source.join("rv/library/loop")).unwrap();

        let staged = prepare(&source).unwrap().unwrap();
        assert!(!staged.path.join("rv/library").exists());
        assert!(staged.path.join("rv/scripts/activate.R").is_file());
        assert!(staged.path.join("inst/rv/keep").is_file());
        assert!(staged.path.join("tools/input").is_file());
        assert!(staged.path.join(".Rbuildignore").is_file());
        assert_eq!(
            fs::read_to_string(staged.path.join("configure")).unwrap(),
            "outside source"
        );
        assert!(
            !fs::symlink_metadata(staged.path.join("configure"))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(staged.path.join("configure"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(
            fs::symlink_metadata(source.join("configure"))
                .unwrap()
                .is_symlink()
        );
        let path = staged.path.clone();
        drop(staged);
        assert!(!path.exists());
    }
}
