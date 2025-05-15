use crate::git::{GitReference, GitRemote};
use crate::library::LocalMetadata;
use crate::lockfile::Source;
use crate::sync::LinkMode;
use crate::sync::errors::SyncError;
use crate::{CommandExecutor, DiskCache, RCmd, ResolvedDependency};
use std::path::Path;

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dir: &Path,
    cache: &DiskCache,
    r_cmd: &impl RCmd,
    git_exec: &(impl CommandExecutor + Clone + 'static),
) -> Result<(), SyncError> {
    let pkg_paths = cache.get_package_paths(&pkg.source, None, None);

    // We will have the source version since we needed to clone it to get the DESCRIPTION file
    if !pkg.installation_status.binary_available() {
        let (repo_url, sha) = match &pkg.source {
            Source::Git { git, sha, .. } => (git.as_str(), sha),
            Source::RUniverse { git, sha, .. } => (git.as_str(), sha),
            _ => unreachable!(),
        };

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

        r_cmd.install(&source_path, library_dir, &pkg_paths.binary)?;
        let metadata = LocalMetadata::Sha(sha.to_owned());
        metadata.write(pkg_paths.binary.join(pkg.name.as_ref()))?;
    }

    // And then we always link the binary folder into the staging library
    LinkMode::new().link_files(&pkg.name, &pkg_paths.binary, library_dir)?;
    Ok(())
}
