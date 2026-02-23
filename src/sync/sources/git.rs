use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use fs_err as fs;

use crate::cache::Cache;
use crate::git::{GitReference, GitRemote};
use crate::library::LocalMetadata;
use crate::lockfile::Source;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{Cancellation, CommandExecutor, RCmd, ResolvedDependency};

#[allow(clippy::too_many_arguments)]
pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dirs: &[&Path],
    cache: &Cache,
    r_cmd: &impl RCmd,
    git_exec: &(impl CommandExecutor + Clone + 'static),
    configure_args: &[String],
    strip: bool,
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let (local_paths, global_paths) = cache.get_package_paths(&pkg.source, None, None);

    // We will have the source version since we needed to clone it to get the DESCRIPTION file
    if !pkg.cache_status.binary_available() {
        let repo_url = pkg.source.git_url().unwrap();
        let sha = pkg.source.sha();
        // TODO: this won't work if multiple projects are trying to checkout different refs
        // on the same user at the same time
        let remote = GitRemote::new(repo_url);
        remote.checkout(
            &local_paths.source,
            &GitReference::Commit(sha),
            git_exec.clone(),
        )?;
        // If we have a directory, don't forget to set it before building it
        let (source_path, sub_dir) = match &pkg.source {
            Source::Git {
                directory: Some(dir),
                ..
            }
            | Source::RUniverse {
                directory: Some(dir),
                ..
            } => (local_paths.source, Some(dir)),
            _ => (local_paths.source, None),
        };

        let output = r_cmd.install(
            &source_path,
            sub_dir,
            library_dirs,
            &local_paths.binary,
            cancellation,
            &pkg.env_vars,
            configure_args,
            strip,
        )?;

        let log_path = cache.local().get_build_log_path(&pkg.source, None, None);
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)?;
            let mut f = fs::File::create(log_path)?;
            f.write_all(output.as_bytes())?;
        }

        let metadata = LocalMetadata::Sha(sha.to_owned());
        metadata.write(local_paths.binary.join(pkg.name.as_ref()))?;
    }

    // Link from global cache if available there, otherwise from local cache
    let binary_path = if pkg.cache_status.global_binary_available() {
        global_paths.unwrap().binary
    } else {
        local_paths.binary
    };

    // And then we always link the binary folder into the staging library
    LinkMode::link_files(None, &pkg.name, binary_path, library_dirs.first().unwrap())?;
    Ok(())
}
