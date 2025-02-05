use std::fs;
use std::fs::Metadata;
use std::path::Path;

use filetime::FileTime;
use walkdir::WalkDir;

const MTIME_FILE: &str = "rv.lock";

/// Copy the whole content of a folder to another folder
pub(crate) fn copy_folder(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
) -> Result<(), std::io::Error> {
    let from = from.as_ref();
    let to = to.as_ref();

    for entry in WalkDir::new(from) {
        let entry = entry?;
        let path = entry.path();

        let relative = path.strip_prefix(from).expect("walkdir starts with root");
        let out_path = to.join(relative);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }

        fs::copy(path, out_path)?;
    }

    Ok(())
}

fn metadata(path: impl AsRef<Path>) -> Result<Metadata, std::io::Error> {
    let path = path.as_ref();
    fs::metadata(path)
}

/// Returns the maximum mtime found in the given folder, looking at all subfolders and
/// following symlinks
/// Taken from cargo crates/cargo-util/src/paths.rs
/// We keep it simple for now and just mtime even if it causes more rebuilds than mtime + hashes
pub(crate) fn mtime_recursive(folder: impl AsRef<Path>) -> Result<FileTime, std::io::Error> {
    let meta = metadata(folder.as_ref())?;
    if !meta.is_file() {
        return Ok(FileTime::from_last_modification_time(&meta));
    }

    // TODO: filter out hidden files/folders?
    let max_mtime = WalkDir::new(folder)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            if e.path_is_symlink() {
                // Use the mtime of both the symlink and its target, to
                // handle the case where the symlink is modified to a
                // different target.
                let sym_meta = match fs::symlink_metadata(e.path()) {
                    Ok(m) => m,
                    Err(err) => {
                        log::debug!(
                            "failed to determine mtime while fetching symlink metadata of {}: {}",
                            e.path().display(),
                            err
                        );
                        return None;
                    }
                };
                let sym_mtime = FileTime::from_last_modification_time(&sym_meta);
                // Walkdir follows symlinks.
                match e.metadata() {
                    Ok(target_meta) => {
                        let target_mtime = FileTime::from_last_modification_time(&target_meta);
                        Some(sym_mtime.max(target_mtime))
                    }
                    Err(err) => {
                        log::debug!(
                            "failed to determine mtime of symlink target for {}: {}",
                            e.path().display(),
                            err
                        );
                        Some(sym_mtime)
                    }
                }
            } else {
                let meta = match e.metadata() {
                    Ok(m) => m,
                    Err(err) => {
                        log::debug!(
                            "failed to determine mtime while fetching metadata of {}: {}",
                            e.path().display(),
                            err
                        );
                        return None;
                    }
                };
                Some(FileTime::from_last_modification_time(&meta))
            }
        })
        .max() // or_else handles the case where there are no files in the directory.
        .unwrap_or_else(|| FileTime::from_last_modification_time(&meta));
    Ok(max_mtime)
}
