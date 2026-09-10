use crate::git::url::GitUrl;
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone)]
enum RemoteType {
    Git,
    GitHub,
    GitLab,
    Bitbucket,
    Svn,
    Cran,
    Url,
    Local,
    Bioc,
}

impl RemoteType {
    fn git_url(&self) -> Option<&'static str> {
        match self {
            RemoteType::GitHub => Some("https://github.com/"),
            RemoteType::GitLab => Some("https://gitlab.com/"),
            RemoteType::Bitbucket => Some("https://bitbucket.org/"),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub enum PackageRemote {
    Git {
        url: GitUrl,
        // Could be a tag, a branch or a commit but we can't know
        // We'll figure it out when cloning the repo later
        // We'll also need to handle the magic `*release`
        reference: Option<String>,
        pull_request: Option<String>,
        directory: Option<String>,
    },
    Url(String),
    // TODO: put more stuff here once we handle bioc in rproject.toml
    Bioc(String),
    Local(String),
    Other(String),
}

// Not for raw git urls, these ones might have a tag/commit/PR associated with it
// Returns `None` if we can't figure out what it is
fn parse_github_like_url(base_url: &str, content: &str) -> Option<(String, PackageRemote)> {
    fn extract_pkg_name_and_directory(text: &str) -> Option<(String, Option<String>)> {
        // We should have 2 elements, eg `owner/repo`
        let split = text.split("/").collect::<Vec<&str>>();
        let mut directory = None;
        if split.len() == 3 {
            directory = Some(split[2].to_string());
        }
        let pkg_name = split.get(1)?.to_string();

        Some((pkg_name, directory))
    }

    let git_url = |path: &str| GitUrl::try_from(format!("{base_url}{path}").as_str()).ok();

    if let Some((path, reference)) = content.split_once("@") {
        let (pkg_name, directory) = extract_pkg_name_and_directory(path)?;

        let remote = PackageRemote::Git {
            url: git_url(path)?,
            reference: Some(reference.to_string()),
            pull_request: None,
            directory,
        };
        Some((pkg_name, remote))
    } else if let Some((path, pull_request)) = content.split_once("#") {
        let (pkg_name, directory) = extract_pkg_name_and_directory(path)?;

        let remote = PackageRemote::Git {
            url: git_url(path)?,
            reference: None,
            pull_request: Some(pull_request.to_string()),
            directory,
        };
        Some((pkg_name, remote))
    } else {
        let (pkg_name, directory) = extract_pkg_name_and_directory(content)?;

        let remote = PackageRemote::Git {
            url: git_url(content)?,
            reference: None,
            pull_request: None,
            directory,
        };
        Some((pkg_name, remote))
    }
}

/// Parses a single entry of a `Remotes:` field.
/// Returns `None` for anything we can't parse
pub(crate) fn parse_remote(content: &str) -> Option<(Option<String>, PackageRemote)> {
    let original = content;
    let mut package_name = String::new();
    let mut content = content;

    // First check if we have an explicit dep name split by `=`
    let parts = content.splitn(2, "=").collect::<Vec<&str>>();
    if parts.len() == 2 {
        package_name = parts[0].to_string();
        content = parts[1];
    }

    // Then the remote type split by `::`
    let parts = content.splitn(2, "::").collect::<Vec<&str>>();
    let remote_type = if parts.len() == 2 {
        content = parts[1];
        match parts[0] {
            "git" => RemoteType::Git,
            "github" => RemoteType::GitHub,
            "gitlab" => RemoteType::GitLab,
            "bitbucket" => RemoteType::Bitbucket,
            "svn" => RemoteType::Svn,
            "url" => RemoteType::Url,
            "local" => RemoteType::Local,
            "bioc" => RemoteType::Bioc,
            // `cran::pkg` means "get it from CRAN", which is what we do by default.
            "cran" => RemoteType::Cran,
            _ => {
                log::warn!("Ignoring remote `{original}`: unknown type `{}`", parts[0]);
                return None;
            }
        }
    } else {
        RemoteType::GitHub
    };

    // Then the rest will depend on the remote type
    let (pkg_name, remote) = match remote_type {
        RemoteType::GitHub | RemoteType::GitLab | RemoteType::Bitbucket => {
            parse_github_like_url(remote_type.git_url()?, content)?
        }
        RemoteType::Git => {
            if content.contains("git@") {
                // If we're there, we should have a `:` in the middle
                let (host, path) = content.split_once(":")?;
                let (pkg_name, remote) = parse_github_like_url(&format!("{host}:"), path)?;
                (pkg_name.trim_end_matches(".git").to_string(), remote)
            } else {
                parse_github_like_url("", content)?
            }
        }
        RemoteType::Svn | RemoteType::Cran => {
            (String::new(), PackageRemote::Other(content.to_string()))
        }
        // Who knows what it could be the package name if you have a URL
        RemoteType::Url => (String::new(), PackageRemote::Url(content.to_string())),
        RemoteType::Bioc => (String::new(), PackageRemote::Bioc(content.to_string())),
        RemoteType::Local => (String::new(), PackageRemote::Local(content.to_string())),
    };

    if package_name.is_empty() {
        Some((
            if !pkg_name.is_empty() {
                Some(pkg_name)
            } else {
                None
            },
            remote,
        ))
    } else {
        Some((Some(package_name), remote))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_remotes() {
        let testcases = vec![
            "r-lib/testthat",
            "r-lib/httr@v0.4",
            "r-lib/testthat@c67018fa4970",
            "klutometis/roxygen#142",
            "github::tidyverse/ggplot2",
            "gitlab::jimhester/covr",
            "git::git@bitbucket.org:djnavarro/lsr.git",
            "git::git@github.com:username/repo.git@a1b2c3d4",
            "git::https://github.com/igraph/rigraph.git@main",
            "bitbucket::sulab/mygene.r@default",
            "bioc::3.3/SummarizedExperiment#117513",
            "svn::https://github.com/tidyverse/stringr",
            "url::https://github.com/tidyverse/stringr/archive/HEAD.zip",
            "local::/pkgs/testthat",
            "clindata=Gilead-BioStats/clindata",
            "yaml=vubiostat/r-yaml",
            "insightsengineering/teal.data",
            "dmlc/xgboost/R-package",
            "cran::dplyr",
        ];

        for t in testcases {
            println!("{t}");
            let (name, remote) = parse_remote(t).expect("a remote we know how to parse");
            insta::with_settings!({
                description => t,
            }, {
                insta::assert_snapshot!(format!("{name:?} => {remote:?}"));
            });
        }
    }

    #[test]
    fn skips_remotes_we_cannot_parse() {
        // Each of those used to panic
        let testcases = vec![
            // No owner, so no repo to clone
            "somepkg",
            // Not a remote type we know
            "gitea::owner/repo",
            // No scheme, so not something GitUrl accepts
            "git::github.com/u/r",
            // No `:` to separate the host from the path
            "git::git@github.com",
        ];

        for t in testcases {
            assert!(parse_remote(t).is_none());
        }
    }
}
