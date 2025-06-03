//! Download and install packages from repositories like CRAN, posit etc

use fs_err as fs;
use std::path::Path;
use std::sync::Arc;

use crate::cache::InstallationStatus;
use crate::http::Http;
use crate::package::PackageType;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{
    Cancellation, DiskCache, HttpDownload, RCmd, ResolvedDependency, get_tarball_urls,
    is_binary_package,
};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dir: &Path,
    cache: &DiskCache,
    r_cmd: &impl RCmd,
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let pkg_paths =
        cache.get_package_paths(&pkg.source, Some(&pkg.name), Some(&pkg.version.original));
    let compile_package = || {
        let source_path = pkg_paths.source.join(pkg.name.as_ref());
        log::debug!("Compiling package from {}", source_path.display());
        r_cmd.install(
            &source_path,
            library_dir,
            &pkg_paths.binary,
            cancellation.clone(),
        )
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

            let tarball_url = get_tarball_urls(pkg, &cache.r_version, &cache.system_info)
                .expect("Dependency has source Repository");
            let http = Http {};

            let download_and_install_source_or_archive = || -> Result<(), SyncError> {
                log::debug!(
                    "Downloading package {} ({}) as source tarball",
                    pkg.name,
                    pkg.version.original
                );
                if let Err(e) =
                    http.download_and_untar(&tarball_url.source, &pkg_paths.source, false)
                {
                    log::warn!(
                        "Failed to download/untar source package from {}: {e:?}, falling back to {}",
                        tarball_url.source,
                        tarball_url.archive
                    );
                    log::debug!(
                        "Downloading package {} ({}) from archive",
                        pkg.name,
                        pkg.version.original
                    );
                    http.download_and_untar(&tarball_url.archive, &pkg_paths.source, false)?;
                }
                compile_package()?;
                Ok(())
            };

            if pkg.kind == PackageType::Source || tarball_url.binary.is_none() {
                download_and_install_source_or_archive()?;
            } else {
                // If we get an error doing the binary download, fall back to source
                if let Err(e) = http.download_and_untar(
                    &tarball_url.binary.clone().unwrap(),
                    &pkg_paths.binary,
                    false,
                ) {
                    log::warn!(
                        "Failed to download/untar binary package from {}: {e:?}, falling back to {}",
                        tarball_url.binary.clone().unwrap(),
                        tarball_url.source
                    );
                    download_and_install_source_or_archive()?;
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
                }
            }
        }
        _ => {}
    }
    // And then we always link the binary folder into the staging library
    LinkMode::new().link_files(&pkg.name, &pkg_paths.binary, library_dir)?;

    Ok(())
}
