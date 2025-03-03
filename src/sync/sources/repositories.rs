//! Download and install packages from repositories like CRAN, posit etc

use std::path::Path;

use fs_err as fs;

use crate::cache::InstallationStatus;
use crate::http::Http;
use crate::package::PackageType;
use crate::r_cmd::InstallError;
use crate::sync::errors::SyncError;
use crate::sync::LinkMode;
use crate::{
    get_tarball_urls, is_binary_package, DiskCache, HttpDownload, PackagePaths, RCmd, ResolvedDependency
};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dir: &Path,
    cache: &DiskCache,
    r_cmd: &impl RCmd,
) -> Result<(), SyncError> {
    let pkg_paths =
        cache.get_package_paths(&pkg.source, Some(&pkg.name), Some(&pkg.version.original));
    install(pkg, library_dir, cache, &pkg_paths, r_cmd)?;
    // And then we always link the binary folder into the staging library
    LinkMode::new().link_files(&pkg.name, &pkg_paths.binary, library_dir)?;

    Ok(())
}

fn install(
    pkg: &ResolvedDependency,
    library_dir: &Path,
    cache: &DiskCache,
    pkg_paths: &PackagePaths,
    r_cmd: &impl RCmd,
) -> Result<(), SyncError> {
    let compile_package = || {
        let source_path = pkg_paths.source.join(pkg.name.as_ref());
        log::debug!("Compiling package from {}", source_path.display());
        r_cmd.install(&source_path, library_dir, &pkg_paths.binary)
    };

    match pkg.installation_status {
        InstallationStatus::Source => {
            log::debug!(
                "Package {} ({}) already present in cache as source but not as binary.",
                pkg.name,
                pkg.version.original
            );
            compile_package()?;
        }
        InstallationStatus::Absent => {
            log::debug!(
                "Package {} ({}) not found in cache, trying to download it.",
                pkg.name,
                pkg.version.original
            );

            let (source_url, binary_url, archive_url) =
                get_tarball_urls(pkg, &cache.r_version, &cache.system_info);
            let http = Http {};

            if pkg.kind == PackageType::Binary && binary_url.is_some() {
                // If we get an error doing the binary download, fall back to source
                if let Err(e) = http.download_and_untar(&binary_url.clone().unwrap(), &pkg_paths.binary, false) {
                    log::warn!("Failed to download/untar binary package from {}: {e:?}, falling back to {source_url}", binary_url.clone().unwrap());
                } else {
                    // Ok we download some tarball. We can't assume it's actually compiled though, it could be just
                    // source files. We have to check first whether what we have is actually binary content.
                    if !is_binary_package(
                        pkg_paths.binary.join(pkg.name.as_ref()),
                        pkg.name.as_ref(),
                    ) {
                        log::debug!("{} was expected as binary, found to be source.", pkg.name);
                        // Move it to the source destination if we don't have it already
                        if pkg_paths.source.is_dir() {
                            fs::remove_dir_all(&pkg_paths.binary)?;
                        } else {
                            fs::create_dir_all(&pkg_paths.source)?;
                            fs::rename(&pkg_paths.binary, &pkg_paths.source)?;
                        }
                        compile_package()?;
                    }
                    return Ok(());
                }
            }

            // Try to install source
            if let Err(e) = http.download_and_untar(&source_url, &pkg_paths.source, false) {
                log::warn!("Failed to download/untar source package from {source_url}: {e:?}, falling back to {archive_url}");
            } else {
                compile_package()?;
                return Ok(());
            }

            // Try to install from archive
            http.download_and_untar(&archive_url, &pkg_paths.source, false)?;
            compile_package()?;
        }
        _ => {}
    }
    Ok(())
}
