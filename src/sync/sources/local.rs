use fs_err as fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use crate::fs::{mtime_recursive, untar_archive};
use crate::library::LocalMetadata;
use crate::lockfile::Source;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{Cancellation, DiskCache, RCmd, ResolvedDependency, is_binary_package};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    project_dir: &Path,
    library_dirs: &[&Path],
    cache: &DiskCache,
    r_cmd: &impl RCmd,
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let (local_path, sha) = match &pkg.source {
        Source::Local { path, sha } => (path, sha.clone()),
        _ => unreachable!(),
    };

    let tempdir = tempfile::tempdir()?;
    let canon_path = fs::canonicalize(project_dir.join(local_path))?;

    let actual_path = if canon_path.is_file() {
        // TODO: we're already untarring in resolve, that's wasteful
        let (path, _) = untar_archive(fs::read(&canon_path)?.as_slice(), tempdir.path(), false)?;
        path.unwrap_or_else(|| canon_path.clone())
    } else {
        canon_path.clone()
    };

    if is_binary_package(&actual_path, pkg.name.as_ref()) {
        log::debug!(
            "Local package in {} is a binary package, copying files to library.",
            actual_path.display()
        );
        LinkMode::Copy.link_files(
            pkg.name.as_ref(),
            &actual_path,
            library_dirs.first().unwrap().join(pkg.name.as_ref()),
        )?;
    } else {
        log::debug!("Building the local package in {}", actual_path.display());
        let output = r_cmd.install(
            &actual_path,
            library_dirs,
            library_dirs.first().unwrap(),
            cancellation,
            &pkg.env_vars,
        )?;

        let log_path = cache.get_build_log_path(&pkg.source, None, None);
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
            let mut f = fs::File::create(log_path)?;
            f.write_all(output.as_bytes())?;
        }
    }

    // If it's a dir, save the dir mtime and if it's a tarball its sha
    let metadata = if canon_path.is_dir() {
        let local_mtime = mtime_recursive(&actual_path)?;
        LocalMetadata::Mtime(local_mtime.unix_seconds())
    } else {
        LocalMetadata::Sha(sha.unwrap())
    };
    metadata.write(library_dirs.first().unwrap().join(pkg.name.as_ref()))?;

    Ok(())
}
