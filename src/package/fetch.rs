use url::Url;

use crate::{
    Cache, CommandExecutor, HttpDownload, ResolvedGitRef, Source,
    git::GitRemote,
    package::{Package, parse_description_file, parse_description_file_in_folder},
};

pub enum FetchDescription<'a, H: HttpDownload, E: CommandExecutor + Clone + 'static> {
    Repository {
        name: &'a str,
        version: &'a str,
        repository: &'a Url,
        downloader: &'a H,
    },
    Git {
        git_url: &'a str,
        directory: Option<&'a str>,
        reference: &'a ResolvedGitRef,
        executor: E,
    },
}

impl<'a, H: HttpDownload, E: CommandExecutor + Clone + 'static> FetchDescription<'a, H, E> {
    pub fn fetch(
        &self,
        cache: &Cache,
        // downloader: &impl HttpDownload,
        // executor: impl CommandExecutor + Clone + 'static,
    ) -> Result<Package, String> {
        match self {
            &Self::Repository {
                name,
                version,
                repository,
                downloader,
            } => {
                let source = Source::Repository {
                    repository: repository.clone(),
                };
                let pkg_paths = cache
                    .local()
                    .get_package_paths(&source, Some(name), Some(version));

                if let Ok(pkg) = parse_description_file_in_folder(&pkg_paths.binary) {
                    return Ok(pkg);
                }
                if let Ok(pkg) = parse_description_file_in_folder(&pkg_paths.source) {
                    return Ok(pkg);
                }

                let mut pkg_url = (*repository).clone();
                {
                    let mut segments = pkg_url.path_segments_mut().expect("Valid absolute url");
                    segments.extend([
                        "src",
                        "contrib",
                        format!("{}_{}.tar.gz", name, version).as_str(),
                    ]);
                }
                let out_dir =
                    match downloader.download_and_untar(&pkg_url, &pkg_paths.source, true, None) {
                        Ok((Some(dir), _)) => dir,
                        Ok((None, _)) => pkg_paths.source.clone(),
                        Err(e) => {
                            return Err(format!(
                                "Failed to download source tarball for {name} from {pkg_url}: {e}"
                            ));
                        }
                    };
                parse_description_file_in_folder(out_dir)
                    .map_err(|e| format!("Failed to parse DESCRIPTION for {name}: {e}"))
            }
            Self::Git {
                git_url,
                directory,
                reference,
                executor,
            } => {
                let clone_path = cache.local().get_git_clone_path(git_url);
                let mut remote = GitRemote::new(git_url);
                if let Some(d) = directory {
                    remote.set_directory(d);
                }

                match remote.sparse_checkout_for_description(
                    clone_path,
                    &reference.as_git_reference(),
                    executor.clone(),
                ) {
                    Ok((_, content)) => parse_description_file(&content)
                        .ok_or(format!("DESCRIPTION file from `{git_url} is not valid`")),
                    Err(e) => Err(format!("Failed to fetch DESCRIPTION from `{git_url}`: {e}")),
                }
            }
        }
    }
}
