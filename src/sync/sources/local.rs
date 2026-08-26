use fs_err as fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::events;
use crate::fs::{mtime_recursive, untar_archive};
use crate::library::LocalMetadata;
use crate::lockfile::Source;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{Cancellation, DiskCache, RCmd, ResolvedDependency, is_binary_package};

#[allow(clippy::too_many_arguments)]
pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    project_dir: &Path,
    library_dirs: &[&Path],
    cache: &DiskCache,
    r_cmd: &impl RCmd,
    configure_args: &[String],
    strip: bool,
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let (local_path, sha, sub_dir) = match &pkg.source {
        Source::Local {
            path,
            sha,
            directory,
        } => (path, sha.clone(), directory.clone()),
        _ => unreachable!(),
    };

    let tempdir = tempfile::tempdir()?;
    let canon_path = fs::canonicalize(project_dir.join(local_path))?;
    // Strip Windows \\?\ extended-length prefix that R can't handle
    let canon_path = PathBuf::from(
        canon_path
            .to_string_lossy()
            .strip_prefix(r"\\?\")
            .unwrap_or(&canon_path.to_string_lossy())
            .to_string(),
    );

    let actual_path = if canon_path.is_file() {
        // TODO: we're already untarring in resolve, that's wasteful
        let (path, _) = untar_archive(fs::read(&canon_path)?.as_slice(), tempdir.path(), false)?;
        path.unwrap_or_else(|| canon_path.clone())
    } else {
        canon_path.clone()
    };

    let pkg_path = match &sub_dir {
        Some(dir) => actual_path.join(dir),
        None => actual_path.clone(),
    };

    if is_binary_package(&pkg_path, pkg.name.as_ref()).map_err(|err| SyncError {
        source: crate::sync::errors::SyncErrorKind::InvalidPackage {
            path: pkg_path.to_path_buf(),
            error: err.to_string(),
        },
    })? {
        log::debug!(
            "Local package in {} is a binary package, copying files to library.",
            pkg_path.display()
        );
        LinkMode::link_files(
            Some(LinkMode::Copy),
            pkg.name.as_ref(),
            &pkg_path,
            library_dirs.first().unwrap().join(pkg.name.as_ref()),
        )?;
    } else {
        let output = if sub_dir.is_none() && canon_path.is_dir() {
            // For plain local directories, run R CMD build first so that .Rbuildignore is respected
            // and extraneous files (like rv/library/) are excluded from the installation.
            log::debug!(
                "Running R CMD build on local package in {}",
                actual_path.display()
            );
            let build_output_dir = tempfile::tempdir()?;
            let tarball_path = r_cmd.build(
                &actual_path,
                build_output_dir.path(),
                library_dirs,
                cancellation.clone(),
                &pkg.env_vars,
            )?;

            // Untar the built tarball and install from the clean source
            let untar_dir = tempfile::tempdir()?;
            let (extracted_path, _) =
                untar_archive(fs::read(&tarball_path)?.as_slice(), untar_dir.path(), false)?;
            let source_path = extracted_path.unwrap_or_else(|| tarball_path.clone());

            log::debug!("Installing built package from {}", source_path.display());
            events::with_task(crate::sync::tasks::compile_task(&pkg.name), || {
                r_cmd.install(
                    &source_path,
                    Option::<&Path>::None,
                    library_dirs,
                    library_dirs.first().unwrap(),
                    cancellation,
                    &pkg.env_vars,
                    configure_args,
                    strip,
                )
            })?
        } else {
            // We have a subdir or an already extracted tarball, we just install.
            // That matches what we do with git, no build step first
            log::debug!(
                "Installing the local package in {} (subdir {:?})",
                actual_path.display(),
                sub_dir
            );
            events::with_task(crate::sync::tasks::compile_task(&pkg.name), || {
                r_cmd.install(
                    &actual_path,
                    sub_dir.as_ref(),
                    library_dirs,
                    library_dirs.first().unwrap(),
                    cancellation,
                    &pkg.env_vars,
                    configure_args,
                    strip,
                )
            })?
        };

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
