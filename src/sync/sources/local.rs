use std::io::Write;
use std::path::Path;

use fs_err as fs;

use crate::consts::LOCAL_MTIME_FILENAME;
use crate::fs::{mtime_recursive, untar_archive};
use crate::sync::errors::SyncError;
use crate::sync::LinkMode;
use crate::{is_binary_package, RCmd, ResolvedDependency};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dir: &Path,
    r_cmd: &impl RCmd,
) -> Result<(), SyncError> {
    // First we check if the package exists in the library and what's the mtime in it
    let local_path = Path::new(pkg.source.source_path()).canonicalize()?;
    let tempdir = tempfile::tempdir()?;

    // TODO: use the file sha somehow?
    let actual_path = if local_path.is_file() {
        // TODO: we're already doing that in resolve, that's wasteful
        let path = untar_archive(std::fs::read(&local_path)?.as_slice(), tempdir.path())?;
        path.unwrap_or_else(|| local_path.clone())
    } else {
        local_path.clone()
    };

    if is_binary_package(&actual_path, pkg.name.as_ref()) {
        log::debug!(
            "Local package in {} is a binary package, copying files to library.",
            actual_path.display()
        );
        LinkMode::Copy.link_files(
            pkg.name.as_ref(),
            actual_path,
            library_dir.join(pkg.name.as_ref()),
        )?;
    } else {
        log::debug!("Building the local package in {}", actual_path.display());
        r_cmd.install(&actual_path, library_dir, &library_dir)?;
    }

    // If it's a dir, save the dir mtime
    if local_path.is_dir() {
        let local_mtime = mtime_recursive(&local_path)?;

        // And just write the mtime in the output directory
        let mut file = fs::File::create(
            library_dir
                .join(pkg.name.as_ref())
                .join(LOCAL_MTIME_FILENAME),
        )?;
        file.write_all(local_mtime.unix_seconds().to_string().as_bytes())?;
    }

    Ok(())
}
