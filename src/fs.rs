use fs_err as fs;
use std::fs::Metadata;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use filetime::FileTime;
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use tar::Archive;
use walkdir::WalkDir;

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

enum ArchiveFormat {
    Zip,
    TarGz,
}

impl ArchiveFormat {
    fn detect<R: Read>(reader: &mut BufReader<R>) -> std::io::Result<Self> {
        // TODO: consider if we should actually try to also check if its a tar.gz
        // and if neither of them actually fail. For now will use the Result
        // signature to give us this option, but for now, it'll never fail
        let buf = reader.fill_buf()?;
        Ok(
            if buf.len() >= 4 && buf.starts_with(&[0x50, 0x4b, 0x03, 0x04]) {
                Self::Zip
            } else {
                Self::TarGz
            },
        )
    }

    fn extract<R: Read>(self, mut reader: BufReader<R>, dest: &Path) -> std::io::Result<()> {
        match self {
            Self::Zip => {
                let mut buffer = Vec::new();
                reader.read_to_end(&mut buffer)?;
                let cursor = std::io::Cursor::new(buffer);
                Ok(zip::read::ZipArchive::new(cursor)?.extract(dest)?)
            }
            Self::TarGz => {
                let tar = GzDecoder::new(reader);
                Archive::new(tar).unpack(dest)
            }
        }
    }
}

/// Untars an archive in the given destination folder, returning a path to the first folder in what
/// was extracted since R tarballs are (always?) a folder
pub(crate) fn untar_archive<R: Read>(
    reader: R,
    dest: impl AsRef<Path>,
) -> Result<Option<PathBuf>, std::io::Error> {
    let dest = dest.as_ref();
    fs::create_dir_all(dest)?;

    let mut buf_reader = BufReader::new(reader);
    let format = ArchiveFormat::detect(&mut buf_reader)?;
    format.extract(buf_reader, dest)?;

    let dir = fs::read_dir(dest)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                Some(entry.path())
            } else {
                None
            }
        })
        .next();

    Ok(dir)
}

pub(crate) fn hash_file(path: impl AsRef<Path>) -> Result<String, std::io::Error> {
    let mut hasher = Sha256::new();
    let file = fs::File::open(path.as_ref())?;
    let mut reader = BufReader::new(file);
    let mut buffer = Vec::new();
    reader.read_to_end(&mut buffer)?;
    hasher.update(&buffer);
    let hash = hasher.finalize();
    Ok(format!("{hash:x}"))
}
