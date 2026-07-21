use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use fs_err as fs;

use crate::cache::Cache;
use crate::events;
use crate::library::LocalMetadata;
use crate::lockfile::Source;
use crate::package::PackageType;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{Cancellation, RCmd, ResolvedDependency};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dirs: &[&Path],
    cache: &Cache,
    r_cmd: &impl RCmd,
    configure_args: &[String],
    strip: bool,
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let (local_paths, global_paths) = cache.get_package_paths(&pkg.source, None, None);

    // Prefer the local source, but copy from the global cache to the local cache if needed.
    // Resolution normally populates the local cache, but the global cache may have been seeded
    // externally and we avoid building directly against the read-only global cache.
    let (url, sha) = match &pkg.source {
        Source::Url { url, sha } => (url, sha),
        _ => unreachable!("install_package called with non-URL source"),
    };
    let source_path = cache
        .get_url_source_path(url, sha)?
        .unwrap_or(local_paths.source);
    let download_path = source_path.join(pkg.name.as_ref());

    // If the downloaded URL archive is already a binary package, copy it into the local binary
    // cache path unless we already have a usable binary (locally or globally).
    if pkg.kind == PackageType::Binary {
        log::debug!(
            "Package from URL in {} is already a binary",
            download_path.display()
        );
        if !pkg.cache_status.binary_available() {
            LinkMode::link_files(
                Some(LinkMode::Copy),
                &pkg.name,
                &source_path,
                &local_paths.binary,
            )?;
        }
    } else {
        log::debug!(
            "Building the package from URL in {}",
            download_path.display()
        );
        let output = events::with_task(crate::sync::tasks::compile_task(&pkg.name), || {
            r_cmd.install(
                &download_path,
                Option::<&Path>::None,
                library_dirs,
                &local_paths.binary,
                cancellation,
                &pkg.env_vars,
                configure_args,
                strip,
            )
        })?;

        let log_path = cache.local().get_build_log_path(&pkg.source, None, None);
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
            let mut f = fs::File::create(log_path)?;
            f.write_all(output.as_bytes())?;
        }
    }

    // Only write metadata to the local binary cache; the global cache is read-only and its
    // binaries are assumed to already contain the correct metadata.
    if !pkg.cache_status.global_binary_available() {
        let metadata = LocalMetadata::Sha(pkg.source.sha().to_owned());
        metadata.write(local_paths.binary.join(pkg.name.as_ref()))?;
    }

    // Link from the global cache if a binary is available there, otherwise from the local cache.
    let binary_path = if pkg.cache_status.global_binary_available() {
        global_paths.unwrap().binary
    } else {
        local_paths.binary
    };

    // And then we always link the binary folder into the staging library
    LinkMode::link_files(None, &pkg.name, &binary_path, library_dirs.first().unwrap())?;

    Ok(())
}
