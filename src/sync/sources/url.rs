use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use fs_err as fs;

use crate::cache::Cache;
use crate::events;
use crate::library::LocalMetadata;
use crate::sync::LinkMode;
use crate::sync::errors::{SyncError, SyncErrorKind};
use crate::{Cancellation, InstallRequest, RCmd, ResolvedDependency, is_binary_package};

#[allow(clippy::too_many_arguments)]
pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dirs: &[&Path],
    cache: &Cache,
    r_cmd: &impl RCmd,
    configure_args: &[String],
    strip: bool,
    cancellation: Arc<Cancellation>,
    sandbox: Option<&Path>,
) -> Result<(), SyncError> {
    let (local_paths, global_paths) = cache.get_package_paths(&pkg.source, None, None);

    if !pkg.cache_status.binary_available() {
        let download_path = local_paths.source.join(pkg.name.as_ref());
        let is_binary = is_binary_package(&download_path, &pkg.name).map_err(|e| SyncError {
            source: SyncErrorKind::InvalidPackage {
                path: download_path.clone(),
                error: e.to_string(),
            },
        })?;

        if is_binary {
            log::debug!(
                "Package from URL in {} is already a binary",
                download_path.display()
            );
            LinkMode::link_files(
                Some(LinkMode::Copy),
                &pkg.name,
                &local_paths.source,
                &local_paths.binary,
            )?;
        } else {
            log::debug!(
                "Building the package from URL in {}",
                download_path.display()
            );
            let output = events::with_task(crate::sync::tasks::compile_task(&pkg.name), || {
                r_cmd.install(
                    InstallRequest {
                        source: &download_path,
                        sub_folder: None,
                        libraries: library_dirs,
                        destination: &local_paths.binary,
                        env_vars: &pkg.env_vars,
                        configure_args,
                        strip,
                        sandbox,
                    },
                    cancellation,
                )
            })?;

            let log_path = cache.local().get_build_log_path(&pkg.source, None, None);
            if let Some(parent) = log_path.parent() {
                fs::create_dir_all(parent)?;
                let mut f = fs::File::create(log_path)?;
                f.write_all(output.as_bytes())?;
            }
        }

        let metadata = LocalMetadata::Sha(pkg.source.sha().to_owned());
        metadata.write(local_paths.binary.join(pkg.name.as_ref()))?;
    }

    let binary_path = if pkg.cache_status.global_binary_available() {
        global_paths.unwrap().binary
    } else {
        local_paths.binary
    };

    // And then we always link the binary folder into the staging library
    LinkMode::link_files(None, &pkg.name, &binary_path, library_dirs.first().unwrap())?;

    Ok(())
}
