use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use fs_err as fs;

use crate::library::LocalMetadata;
use crate::package::PackageType;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{Cancellation, DiskCache, RCmd, ResolvedDependency};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dirs: &[&Path],
    cache: &DiskCache,
    r_cmd: &impl RCmd,
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let pkg_paths = cache.get_package_paths(&pkg.source, None, None);
    let download_path = pkg_paths.source.join(pkg.name.as_ref());

    // If we have a binary, copy it since we don't keep cache around for binary URL packages
    if pkg.kind == PackageType::Binary {
        log::debug!(
            "Package from URL in {} is already a binary",
            download_path.display()
        );
        if !pkg_paths.binary.is_dir() {
            LinkMode::Copy.link_files(&pkg.name, &pkg_paths.source, &pkg_paths.binary)?;
        }
    } else {
        log::debug!(
            "Building the package from URL in {}",
            download_path.display()
        );
        let output = r_cmd.install(
            &download_path,
            library_dirs,
            &pkg_paths.binary,
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

    let metadata = LocalMetadata::Sha(pkg.source.sha().to_owned());
    metadata.write(pkg_paths.binary.join(pkg.name.as_ref()))?;

    // And then we always link the binary folder into the staging library
    LinkMode::new().link_files(&pkg.name, &pkg_paths.binary, library_dirs.first().unwrap())?;

    Ok(())
}
