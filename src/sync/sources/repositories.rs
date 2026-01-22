//! Download and install packages from repositories like CRAN, posit etc

use fs_err as fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use crate::cache::{Cache, InstallationStatus};
use crate::consts::BUILT_FROM_SOURCE_FILENAME;
use crate::http::Http;
use crate::package::PackageType;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{
    Cancellation, HttpDownload, RCmd, ResolvedDependency, get_tarball_urls, is_binary_package,
};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dirs: &[&Path],
    cache: &Cache,
    r_cmd: &impl RCmd,
    configure_args: &[String],
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let (local_paths, global_paths) =
        cache.get_package_paths(&pkg.source, Some(&pkg.name), Some(&pkg.version.original));

    let compile_package = || -> Result<(), SyncError> {
        let source_path = local_paths.source.join(pkg.name.as_ref());
        log::debug!("Compiling package from {}", source_path.display());
        match r_cmd.install(
            &source_path,
            Option::<&Path>::None,
            library_dirs,
            &local_paths.binary,
            cancellation.clone(),
            &pkg.env_vars,
            configure_args,
        ) {
            Ok(output) => {
                // not using the path for the cache
                let log_path = cache.local().get_build_log_path(
                    &pkg.source,
                    Some(pkg.name.as_ref()),
                    Some(&pkg.version.original),
                );
                if let Some(parent) = log_path.parent() {
                    fs::create_dir_all(parent)?;
                    let mut f = fs::File::create(log_path)?;
                    f.write_all(output.as_bytes())?;
                }
                // Create the marker file for local compilation
                let _ = fs::File::create(
                    local_paths
                        .binary
                        .join(pkg.name.as_ref())
                        .join(BUILT_FROM_SOURCE_FILENAME),
                )?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    };

    // If the binary is not available, check the status to either download and/or compile
    if !pkg.cache_status.binary_available() {
        match pkg.cache_status.local {
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

                let tarball_url = get_tarball_urls(pkg, cache.r_version(), cache.system_info())
                    .expect("Dependency has source Repository");
                let http = Http {};

                let download_and_install_source_or_archive = || -> Result<(), SyncError> {
                    log::debug!(
                        "Downloading package {} ({}) as source tarball",
                        pkg.name,
                        pkg.version.original
                    );
                    if let Err(e) = http.download_and_untar(
                        &tarball_url.source,
                        &local_paths.source,
                        false,
                        None,
                    ) {
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
                        http.download_and_untar(
                            &tarball_url.archive,
                            &local_paths.source,
                            false,
                            None,
                        )?;
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
                        &local_paths.binary,
                        false,
                        None,
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
                        let bin_path = local_paths.binary.join(pkg.name.as_ref());
                        if !is_binary_package(&bin_path, pkg.name.as_ref()).map_err(|err| {
                            SyncError {
                                source: crate::sync::errors::SyncErrorKind::InvalidPackage {
                                    path: bin_path,
                                    error: err.to_string(),
                                },
                            }
                        })? {
                            log::debug!("{} was expected as binary, found to be source.", pkg.name);
                            // Move it to the source destination if we don't have it already
                            if local_paths.source.is_dir() {
                                fs::remove_dir_all(&local_paths.binary)?;
                            } else {
                                fs::create_dir_all(&local_paths.source)?;
                                fs::rename(&local_paths.binary, &local_paths.source)?;
                            }
                            compile_package()?;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // And then we always link the binary folder into the staging library
    LinkMode::link_files(
        None,
        &pkg.name,
        if pkg.cache_status.global_binary_available() {
            global_paths.unwrap().binary
        } else {
            local_paths.binary
        },
        library_dirs.first().unwrap(),
    )?;

    Ok(())
}
