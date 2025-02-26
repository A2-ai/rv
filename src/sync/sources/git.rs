use std::path::Path;

use crate::git::GitReference;
use crate::lockfile::Source;
use crate::sync::errors::SyncError;
use crate::sync::LinkMode;
use crate::{DiskCache, GitOperations, RCmd, ResolvedDependency};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dir: &Path,
    cache: &DiskCache,
    r_cmd: &impl RCmd,
    git_ops: &impl GitOperations,
) -> Result<(), SyncError> {
    let pkg_paths = cache.get_package_paths(&pkg.source, None, None);

    // We will have the source version since we needed to clone it to get the DESCRIPTION file
    if !pkg.installation_status.binary_available() {
        let (repo_url, sha) = match &pkg.source {
            Source::Git { git, sha, .. } => (git, sha),
            _ => unreachable!(),
        };

        // TODO: this won't work if multiple projects are trying to checkout different refs
        // on the same user at the same time
        git_ops.clone_and_checkout(
            repo_url,
            Some(GitReference::Commit(sha)),
            &pkg_paths.source,
        )?;
        // If we have a directory, don't forget to set it before building it
        let source_path = match &pkg.source {
            Source::Git {
                directory: Some(dir),
                ..
            } => pkg_paths.source.join(dir),
            _ => pkg_paths.source,
        };

        r_cmd.install(&source_path, library_dir, &pkg_paths.binary)?;
    }

    // And then we always link the binary folder into the staging library
    LinkMode::new().link_files(&pkg.name, &pkg_paths.binary, library_dir)?;
    Ok(())
}
