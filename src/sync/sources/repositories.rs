//! Download and install packages from repositories like CRAN, posit etc

use fs_err as fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use url::Url;

use crate::cache::{Cache, InstallationStatus};
use crate::consts::BUILT_FROM_SOURCE_FILENAME;
use crate::http::Http;
use crate::package::PackageType;
use crate::repository_urls::TarballUrls;
use crate::sync::LinkMode;
use crate::sync::errors::{SyncError, SyncErrorKind};
use crate::{
    Cancellation, HttpDownload, PackagePaths, RCmd, ResolvedDependency, get_tarball_urls,
    is_binary_package,
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

                if let PackageType::Source =
                    download_package(&http, &tarball_url, &local_paths, pkg)?
                {
                    compile_package()?;
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

fn download_package(
    http: &impl HttpDownload,
    urls: &TarballUrls,
    local_paths: &PackagePaths,
    pkg: &ResolvedDependency,
) -> Result<PackageType, SyncError> {
    // 1. Download Binary if possible/requested
    if let Some(binary_url) = &urls.binary
        && pkg.kind == PackageType::Binary
    {
        match try_download_package(http, binary_url, &local_paths, pkg.name.as_ref(), true) {
            Ok(pkg_type) => return Ok(pkg_type),
            Err(e) => {
                log::warn!(
                    "Failed to download binary from {}: {}. Trying source",
                    binary_url,
                    e
                );
            }
        }
    }

    // 2. Download Source
    match try_download_package(http, &urls.source, &local_paths, pkg.name.as_ref(), false) {
        Ok(pkg_type) => return Ok(pkg_type),
        Err(e) => {
            log::warn!(
                "Failed to download source from {}: {}. Trying {}archive",
                &urls.source,
                e,
                if urls.binary_archive.is_some() && !pkg.force_source {
                    "binary "
                } else {
                    ""
                }
            );
        }
    }

    // 3. Download binary from archive
    if let Some(binary_archive_url) = &urls.binary_archive
        && !pkg.force_source
    {
        match try_download_package(
            http,
            binary_archive_url,
            &local_paths,
            pkg.name.as_ref(),
            true,
        ) {
            Ok(pkg_type) => return Ok(pkg_type),
            Err(e) => {
                log::warn!(
                    "Failed to download binary archive from {}: {}. Trying archive",
                    binary_archive_url,
                    e
                );
            }
        }
    }

    // 4. Download source from archive
    try_download_package(
        http,
        &urls.source_archive,
        &local_paths,
        pkg.name.as_ref(),
        false,
    )
}

fn try_download_package(
    http: &impl HttpDownload,
    url: &Url,
    local_paths: &PackagePaths,
    pkg_name: &str,
    expect_binary: bool,
) -> Result<PackageType, SyncError> {
    let mut pkg_type = PackageType::Source;

    let destination = if expect_binary {
        &local_paths.binary
    } else {
        &local_paths.source
    };

    http.download_and_untar(url, destination, false, None)?;

    if expect_binary {
        let pkg_path = destination.join(pkg_name);
        if !is_binary_package(&pkg_path, pkg_name).map_err(|e| SyncError {
            source: SyncErrorKind::InvalidPackage {
                path: pkg_path,
                error: e.to_string(),
            },
        })? {
            log::debug!("{} was expected as binary, found to be source", pkg_name);
            // Move it to the source destination if we don't have it already
            if local_paths.source.is_dir() {
                fs::remove_dir_all(&local_paths.binary)?;
            } else {
                fs::create_dir_all(&local_paths.source)?;
                fs::rename(&local_paths.binary, &local_paths.source)?;
            }
        } else {
            pkg_type = PackageType::Binary;
        }
    }

    Ok(pkg_type)
}
