use std::error::Error;

use url::Url;

use crate::{
    Cache, CommandExecutor, HttpDownload, ResolvedGitRef, Source,
    git::GitRemote,
    package::{Package, parse_description_file, parse_description_file_in_folder},
};

pub enum FetchPackage<'a, H: HttpDownload, E: CommandExecutor + Clone + 'static> {
    Repository {
        name: &'a str,
        version: &'a str,
        repository: &'a Url,
        downloader: &'a H,
    },
    Git {
        git_url: &'a str,
        reference: &'a ResolvedGitRef,
        directory: Option<&'a str>,
        executor: E,
    },
}

impl<'a, H: HttpDownload, E: CommandExecutor + Clone + 'static> FetchPackage<'a, H, E> {
    pub fn fetch(&self, cache: &Cache) -> Result<Package, Box<dyn Error>> {
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
                if let Ok(pkg) = parse_description_file_in_folder(&pkg_paths.binary)
                    .or(parse_description_file_in_folder(&pkg_paths.source))
                {
                    return Ok(pkg);
                }

                let mut pkg_url = repository.clone();
                {
                    let mut segments = pkg_url.path_segments_mut().expect("Valid absolute url");
                    segments.extend([
                        "src",
                        "contrib",
                        format!("{name}_{version}.tar.gz").as_str(),
                    ]);
                }

                let (out_dir, _) = downloader
                    .download_and_untar(&pkg_url, &pkg_paths.source, true, None)
                    .map_err(|e| {
                        format!("Failed to download source tarball for {name} from {pkg_url}: {e}")
                    })?;
                let pkg_dir = out_dir.as_ref().unwrap_or(&pkg_paths.source);
                parse_description_file_in_folder(pkg_dir)
            }
            Self::Git {
                git_url,
                reference,
                directory,
                executor,
            } => {
                let clone_path = cache.local().get_git_clone_path(git_url);
                let mut remote = GitRemote::new(git_url);
                if let Some(d) = directory {
                    remote.set_directory(d);
                }

                let (_, content) = remote
                    .sparse_checkout_for_description(
                        &clone_path,
                        &reference.as_git_reference(),
                        executor.clone(),
                    )
                    .map_err(|e| {
                        format!("Failed to fetch DESCRIPTION file from `{git_url}`: {e}")
                    })?;
                parse_description_file(&content)
                    .ok_or(format!("DESCRIPTION file from `{git_url}` is not valid").into())
            }
        }
    }
}
