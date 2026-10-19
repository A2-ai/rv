//! Prepare ordinary local packages without walking state that never belongs in them.
//!
//! R CMD build copies the entire source (dereferencing symlinks) before applying
//! .Rbuildignore. The rv project directory (notably rv/library, which links every
//! installed dependency), VCS metadata and Python virtual environments are never
//! package content, so they are pruned unconditionally before R sees the source.
//! Every other exclusion is left to R, since ignored files can be build-script inputs.

use std::path::{Path, PathBuf};

use fs_err as fs;
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::consts::RV_DIR_NAME;
use crate::fs::preserve_mtime;

/// Top-level entries of a package source that are never bundled.
const PRUNED_ENTRIES: [&str; 3] = [RV_DIR_NAME, ".git", ".venv"];

/// The guard owns the scratch tree until R has finished reading it.
pub(crate) struct BuildSource {
    _scratch: TempDir,
    pub path: PathBuf,
}

pub(crate) fn prepare(source: &Path) -> Result<Option<BuildSource>, std::io::Error> {
    let pruned: Vec<PathBuf> = PRUNED_ENTRIES
        .iter()
        .map(|name| source.join(name))
        .filter(|path| path.symlink_metadata().is_ok())
        .collect();
    if pruned.is_empty() {
        return Ok(None);
    }

    let scratch = tempfile::tempdir()?;
    let path = scratch.path().join(source.file_name().unwrap_or_default());
    copy_source(source, &path, &pruned)?;
    log::debug!(
        "Staged {} in {}, pruning {:?}",
        source.display(),
        path.display(),
        pruned
    );
    Ok(Some(BuildSource {
        _scratch: scratch,
        path,
    }))
}

fn copy_source(
    source: &Path,
    destination: &Path,
    pruned: &[PathBuf],
) -> Result<(), std::io::Error> {
    let mut directories = Vec::new();
    // Like R's cp -LR, materialize retained symlinks. Preserving relative links
    // while relocating the source would silently change their targets.
    for entry in WalkDir::new(source)
        .follow_links(true)
        .into_iter()
        .filter_entry(|entry| !pruned.iter().any(|path| path == entry.path()))
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
    fn leaves_sources_without_pruned_entries_to_r() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir_all(source.path().join("R")).unwrap();
        fs::create_dir_all(source.path().join("inst/rv")).unwrap();
        assert!(prepare(source.path()).unwrap().is_none());
    }

    #[test]
    fn prunes_regardless_of_rbuildignore() {
        let source = tempfile::tempdir().unwrap();
        fs::create_dir_all(source.path().join("R")).unwrap();
        fs::create_dir_all(source.path().join(".venv/bin")).unwrap();
        // A worktree or submodule has a .git file rather than a directory.
        fs::write(source.path().join(".git"), "gitdir: elsewhere").unwrap();
        let staged = prepare(source.path()).unwrap().unwrap();
        assert!(staged.path.join("R").is_dir());
        assert!(!staged.path.join(".venv").exists());
        assert!(!staged.path.join(".git").exists());
    }

    #[cfg(unix)]
    #[test]
    fn prunes_before_traversal_and_materializes_retained_relative_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("pkg");
        fs::create_dir_all(source.join("rv/library")).unwrap();
        fs::create_dir_all(source.join("rv/scripts")).unwrap();
        fs::create_dir_all(source.join(".git/objects")).unwrap();
        fs::create_dir_all(source.join("inst/rv")).unwrap();
        fs::create_dir_all(source.join("tools")).unwrap();
        fs::write(source.join(".Rbuildignore"), "^tools$\n").unwrap();
        fs::write(source.join("tools/input"), "build input").unwrap();
        fs::write(source.join("inst/rv/keep"), "nested rv").unwrap();
        fs::write(source.join("rv/scripts/activate.R"), "rv helper").unwrap();
        fs::write(root.path().join("shared"), "outside source").unwrap();
        fs::set_permissions(root.path().join("shared"), Permissions::from_mode(0o755)).unwrap();
        symlink("../shared", source.join("configure")).unwrap();
        // A walk entering the pruned library would fail on this cycle.
        symlink(".", source.join("rv/library/loop")).unwrap();

        let staged = prepare(&source).unwrap().unwrap();
        assert!(!staged.path.join("rv").exists());
        assert!(!staged.path.join(".git").exists());
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
