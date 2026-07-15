use std::error::Error;
use std::path::PathBuf;

use url::Url;

use crate::{
    Cache, CommandExecutor, HttpDownload, ResolvedGitRef, Source,
    git::GitRemote,
    package::{Package, PackageType, parse_description_file, parse_description_file_in_folder},
    repository_urls::get_tarball_urls_from_parts,
};

pub enum FetchPackage<'a, H: HttpDownload, E: CommandExecutor + Clone + 'static> {
    Repository {
        name: &'a str,
        version: &'a str,
        repository: &'a Url,
        package_type: PackageType,
        path: Option<&'a str>,
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
                package_type,
                path,
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

                let urls = get_tarball_urls_from_parts(
                    repository,
                    name,
                    version,
                    path,
                    cache.r_version(),
                    cache.system_info(),
                );
                let mut attempts: Vec<(&Url, &PathBuf)> = Vec::new();
                if package_type == PackageType::Binary {
                    if let Some(url) = urls.binary.as_ref() {
                        attempts.push((url, &pkg_paths.binary));
                    }
                    if let Some(url) = urls.binary_archive.as_ref() {
                        attempts.push((url, &pkg_paths.binary));
                    }
                }
                attempts.push((&urls.source, &pkg_paths.source));
                attempts.push((&urls.source_archive, &pkg_paths.source));

                let mut last_err = None;
                for (url, dest) in attempts {
                    match downloader.download_and_untar(url, dest, true, None) {
                        Ok((out_dir, _)) => {
                            let pkg_dir = out_dir.as_ref().unwrap_or(dest);
                            return parse_description_file_in_folder(pkg_dir);
                        }
                        Err(e) => {
                            log::debug!("Failed to fetch {name} {version} from {url}: {e}");
                            last_err = Some(e.to_string());
                        }
                    }
                }

                Err(format!(
                    "Failed to download {name} {version} tarball from any repository URL: {}",
                    last_err.unwrap_or_default()
                )
                .into())
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
