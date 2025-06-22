use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use fs_err as fs;

use crate::git::{GitReference, GitRemote};
use crate::library::LocalMetadata;
use crate::lockfile::Source;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{Cancellation, CommandExecutor, DiskCache, RCmd, ResolvedDependency};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dirs: &[&Path],
    cache: &DiskCache,
    r_cmd: &impl RCmd,
    git_exec: &(impl CommandExecutor + Clone + 'static),
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let pkg_paths = cache.get_package_paths(&pkg.source, None, None);

    // We will have the source version since we needed to clone it to get the DESCRIPTION file
    if !pkg.installation_status.binary_available() {
        let repo_url = pkg.source.git_url().unwrap();
        let sha = pkg.source.sha();
        // TODO: this won't work if multiple projects are trying to checkout different refs
        // on the same user at the same time
        let remote = GitRemote::new(repo_url);
        remote.checkout(
            &pkg_paths.source,
            &GitReference::Commit(sha),
            git_exec.clone(),
        )?;
        // If we have a directory, don't forget to set it before building it
        let source_path = match &pkg.source {
            Source::Git {
                directory: Some(dir),
                ..
            }
            | Source::RUniverse {
                directory: Some(dir),
                ..
            } => pkg_paths.source.join(dir),
            _ => pkg_paths.source,
        };

        let output = r_cmd.install(
            &source_path,
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

        let metadata = LocalMetadata::Sha(sha.to_owned());
        metadata.write(pkg_paths.binary.join(pkg.name.as_ref()))?;
    }

    // And then we always link the binary folder into the staging library
    LinkMode::new().link_files(&pkg.name, &pkg_paths.binary, library_dirs.first().unwrap())?;
    Ok(())
}
