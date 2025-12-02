use fs_err as fs;
use std::fs::Metadata;
use std::io::Read;
use std::path::{Path, PathBuf};

use filetime::FileTime;
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;
use walkdir::WalkDir;

#[cfg(feature = "cli")]
use rayon::prelude::*;

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
        } else {
            fs::copy(path, out_path)?;
        }
    }

    Ok(())
}

/// Copy the whole content of a folder to another folder using parallel processing
/// This is optimized for NFS scenarios where parallel I/O can improve performance
/// Thread count can be configured via the RV_COPY_THREADS environment variable
#[cfg(feature = "cli")]
fn copy_folder_parallel(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    default_num_threads: usize,
) -> Result<(), std::io::Error> {
    use crate::consts::COPY_THREADS_ENV_VAR_NAME;

    let num_threads = std::env::var(COPY_THREADS_ENV_VAR_NAME)
        .ok()
        .and_then(|x| x.parse::<usize>().ok())
        .unwrap_or(default_num_threads);

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build()
        .map_err(std::io::Error::other)?;

    let from = from.as_ref();
    let to = to.as_ref();

    // Collect all entries and copy them in parallel
    let entries: Result<Vec<_>, _> = WalkDir::new(from)
        .into_iter()
        .collect::<Result<Vec<_>, _>>();
    let entries = entries?;

    pool.install(|| {
        entries.par_iter().try_for_each(|entry| {
            let path = entry.path();
            let relative = path.strip_prefix(from).expect("walkdir starts with root");
            let out_path = to.join(relative);

            if entry.file_type().is_dir() {
                fs::create_dir_all(&out_path)?;
            } else {
                // Ensure parent directory exists before copying file
                if let Some(parent) = out_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(path, out_path)?;
            }
            Ok::<(), std::io::Error>(())
        })
    })?;

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
    if !meta.is_dir() {
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

/// Untars an archive in the given destination folder, returning a path to the first folder in what
/// was extracted since R tarballs are (always?) a folder
/// For windows binaries, they are in .zip archives and will be unzipped
pub(crate) fn untar_archive<R: Read>(
    mut reader: R,
    dest: impl AsRef<Path>,
    compute_hash: bool,
) -> Result<(Option<PathBuf>, Option<String>), std::io::Error> {
    let dest = dest.as_ref();
    fs::create_dir_all(dest)?;

    let mut hash = None;
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;
    if compute_hash {
        let mut hasher = Sha256::new();
        hasher.update(&buffer);
        let hash_out = hasher.finalize();
        hash = Some(format!("{hash_out:x}"));
    }

    match buffer[..4] {
        // zip
        [0x50, 0x4b, 0x03, 0x04] => {
            // zip lib requires Seek
            let cursor = std::io::Cursor::new(buffer);
            zip::read::ZipArchive::new(cursor)?.extract(dest)?;
        }
        // tar.gz, .tgz
        [0x1F, 0x8B, ..] => {
            // If we are on NFS on Linux and using the CLI, we will untar in /dev/shm and
            // then copy the package in parallel to the destination if there are more than `n` files.
            // For smaller packages, we keep doing a serial copy.
            // In all other cases we do a normal untar in destination
            let use_nfs_optimization =
                cfg!(feature = "cli") && Path::new("/dev/shm").exists() && is_nfs(dest)?;
            let mut done = false;

            if use_nfs_optimization {
                log::debug!("Using NFS optimization for untarring archive");

                match tempfile::tempdir_in("/dev/shm") {
                    Ok(temp_dir) => {
                        let tar = GzDecoder::new(buffer.as_slice());
                        let mut archive = Archive::new(tar);
                        if archive.unpack(temp_dir.path()).is_ok() {
                            // Count files to determine if we should use parallel copy
                            let file_count = WalkDir::new(temp_dir.path())
                                .into_iter()
                                .filter_map(|e| e.ok())
                                .filter(|e| e.file_type().is_file())
                                .count();

                            // Magic number only used once, looks good though
                            if file_count < 50 {
                                log::debug!("Too few files ({file_count}), using sequential copy",);
                                copy_folder(temp_dir.path(), dest)?;
                            } else {
                                #[cfg(feature = "cli")]
                                {
                                    let default_num_threads = if file_count < 1000 {
                                        4
                                    } else if file_count < 5000 {
                                        8
                                    } else {
                                        // This might only be BH?
                                        16
                                    };
                                    log::debug!(
                                        "{file_count} files found, using parallel copy with a default of {default_num_threads} threads",
                                    );
                                    copy_folder_parallel(
                                        temp_dir.path(),
                                        dest,
                                        default_num_threads,
                                    )?;
                                }
                            }

                            done = true;
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "Failed to create temp dir in /dev/shm, falling back to direct extraction: {err}"
                        );
                    }
                }
            }

            if !done {
                // Direct extraction as fallback
                let tar = GzDecoder::new(buffer.as_slice());
                let mut archive = Archive::new(tar);
                archive.unpack(dest)?;
            }
        }
        _ => {
            return Err(std::io::Error::other("not tar.gz or a .zip archive"));
        }
    }

    let dir: Option<PathBuf> = fs::read_dir(dest)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                Some(entry.path())
            } else {
                None
            }
        })
        .next();

    Ok((dir, hash))
}

#[cfg(target_os = "linux")]
pub(crate) fn is_nfs(path: impl AsRef<Path>) -> Result<bool, std::io::Error> {
    use nix::sys::statfs::{NFS_SUPER_MAGIC, statfs};
    let st = statfs(path.as_ref()).map_err(std::io::Error::other)?;

    Ok(st.filesystem_type() == NFS_SUPER_MAGIC)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn is_nfs(_path: impl AsRef<Path>) -> std::io::Result<bool> {
    Ok(false)
}
