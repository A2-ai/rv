use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Clone)]
enum RemoteType {
    Git,
    GitHub,
    GitLab,
    Bitbucket,
    Svn,
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
        url: String,
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

// Not for raw git urls.
// These ones might have a tag/commit/PR associated with it
fn parse_github_like_url(base_url: &str, content: &str) -> (String, PackageRemote) {
    fn extract_pkg_name_and_directory(text: &str) -> (String, Option<String>) {
        // We should have 2 elements
        let split = text.split("/").collect::<Vec<&str>>();
        let mut directory = None;
        if split.len() == 3 {
            directory = Some(split[2].to_string());
        }
        let pkg_name = split[1].to_string();

        (pkg_name, directory)
    }

    if content.contains("@") {
        let parts = content.splitn(2, "@").collect::<Vec<&str>>();
        let (pkg_name, directory) = extract_pkg_name_and_directory(parts[0]);

        let remote = PackageRemote::Git {
            url: format!("{}{}", base_url, parts[0]),
            reference: Some(parts[1].to_string()),
            pull_request: None,
            directory,
        };
        (pkg_name, remote)
    } else if content.contains("#") {
        let parts = content.splitn(2, "#").collect::<Vec<&str>>();
        let (pkg_name, directory) = extract_pkg_name_and_directory(parts[0]);

        let remote = PackageRemote::Git {
            url: format!("{}{}", base_url, parts[0]),
            reference: None,
            pull_request: Some(parts[1].to_string()),
            directory,
        };
        (pkg_name, remote)
    } else {
        let (pkg_name, directory) = extract_pkg_name_and_directory(content);

        let remote = PackageRemote::Git {
            url: format!("{}{}", base_url, content),
            reference: None,
            pull_request: None,
            directory,
        };
        (pkg_name, remote)
    }
}

pub(crate) fn parse_remote(content: &str) -> (Option<String>, PackageRemote) {
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
            _ => unreachable!("Unknown remote type: {}", parts[0]),
        }
    } else {
        RemoteType::GitHub
    };

    // Then the rest will depend on the remote type
    let (pkg_name, remote) = match remote_type {
        RemoteType::GitHub | RemoteType::GitLab | RemoteType::Bitbucket => {
            parse_github_like_url(remote_type.git_url().unwrap(), content)
        }
        RemoteType::Git => {
            if content.contains("git@") {
                // If we're there, we should have a `:` in the middle
                let parts = content.splitn(2, ":").collect::<Vec<&str>>();
                let (pkg_name, remote) = parse_github_like_url(&format!("{}:", parts[0]), parts[1]);
                (pkg_name.trim_end_matches(".git").to_string(), remote)
            } else {
                parse_github_like_url("", content)
            }
        }
        RemoteType::Svn => (String::new(), PackageRemote::Other(content.to_string())),
        // Who knows what it could be the package name if you have a URL
        RemoteType::Url => (String::new(), PackageRemote::Url(content.to_string())),
        RemoteType::Bioc => (String::new(), PackageRemote::Bioc(content.to_string())),
        RemoteType::Local => (String::new(), PackageRemote::Local(content.to_string())),
    };

    if package_name.is_empty() {
        (
            if !pkg_name.is_empty() {
                Some(pkg_name)
            } else {
                None
            },
            remote,
        )
    } else {
        (Some(package_name), remote)
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
        ];

        for t in testcases {
            let (name, remote) = parse_remote(t);
            insta::with_settings!({
                description => t,
            }, {
                insta::assert_snapshot!(format!("{name:?} => {remote:?}"));
            });
        }
    }
}
