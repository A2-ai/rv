use std::{
    io::{self, Read},
    path::Path,
    process::Command,
};

use fs_err::write;
use url::Url;

use crate::{Repository, consts::LIBRARY_ROOT_DIR_NAME};

const GITIGNORE_PATH: &str = "rv/.gitignore";
const LIBRARY_PATH: &str = "rv/library";
const CONFIG_FILENAME: &str = "rproject.toml";

const INITIAL_CONFIG: &str = r#"[project]
name = "%project_name%"
r_version = "%r_version%"

# A list of repositories to fetch packages from. Order matters: we will try to get a package from each repository in order.
# The alias is only used in this file if you want to specifically require a dependency to come from a certain repository.
# Example: { alias = "PPM", url = "https://packagemanager.posit.co/cran/latest" },
repositories = [
%repositories%
]

# A list of packages to install and any additional configuration
# Examples:
    # "dplyr",
    # {name = "dplyr", repository = "CRAN"},
    # {name = "dplyr", git = "https://github.com/tidyverse/dplyr.git", tag = "v1.1.4"},
dependencies = [
%dependencies%
]
"#;

/// This function initializes a given directory to be an rv project. It does this by:
/// - Creating the directory if it does not exist
/// - Creating the library directory if it does not exist (<path/to/directory>/rv/library)
///     - If a library directory exists, init will not create a new one or remove any of the installed packages
/// - Creating a .gitignore file within the rv subdirectory to prevent upload of installed packages to git
/// - Initialize the config file with the R version and repositories set as options within R
/// - Activate the project by setting the libPaths to the rv library
pub fn init(
    project_directory: impl AsRef<Path>,
    r_version: &str,
    repositories: &[Repository],
    dependencies: &[String],
    force: bool,
) -> Result<(), InitError> {
    let proj_dir = project_directory.as_ref();
    init_structure(proj_dir)?;
    let config_path = proj_dir.join(CONFIG_FILENAME);
    if config_path.exists() && !force {
        return Ok(());
    }
    let project_name = proj_dir
        .canonicalize()
        .map_err(|e| InitError {
            source: InitErrorKind::Io(e),
        })?
        .iter()
        .next_back()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or("my rv project".to_string());

    let config = render_config(&project_name, r_version, repositories, dependencies);

    write(proj_dir.join(CONFIG_FILENAME), config)?;
    Ok(())
}

fn render_config(
    project_name: &str,
    r_version: &str,
    repositories: &[Repository],
    dependencies: &[String],
) -> String {
    let repos = repositories
        .iter()
        .map(|r| format!(r#"    {{alias = "{}", url = "{}"}},"#, r.alias, r.url()))
        .collect::<Vec<_>>()
        .join("\n");

    let deps = dependencies
        .iter()
        .map(|d| format!(r#"    "{d}","#))
        .collect::<Vec<_>>()
        .join("\n");

    INITIAL_CONFIG
        .replace("%project_name%", project_name)
        .replace("%r_version%", r_version)
        .replace("%repositories%", &repos)
        .replace("%dependencies%", &deps)
}

pub fn find_r_repositories() -> Result<Vec<Repository>, InitError> {
    let r_code = r#"
    repos <- getOption("repos")
    cat(paste(names(repos), repos, sep = "\t", collapse = "\n"))
    "#;

    let (mut recv, send) = std::io::pipe().map_err(|e| InitError {
        source: InitErrorKind::Command(e),
    })?;

    let mut command = Command::new("Rscript");
    command
        .arg("-e")
        .arg(r_code)
        .stdout(send.try_clone().map_err(|e| InitError {
            source: InitErrorKind::Command(e),
        })?)
        .stderr(send);

    let mut handle = command.spawn().map_err(|e| InitError {
        source: InitErrorKind::Command(e),
    })?;

    drop(command);

    let mut output = String::new();
    recv.read_to_string(&mut output).unwrap();
    let status = handle.wait().unwrap();

    if !status.success() {
        return Err(InitError {
            source: InitErrorKind::CommandFailed(output),
        });
    }

    Ok(output
        .as_str()
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let alias = parts.next()?.to_string();
            let url = strip_linux_url(parts.next()?);
            if let Ok(url) = Url::parse(&url) {
                Some(Repository::new(alias, url, false))
            } else {
                None
            }
        })
        .collect::<Vec<_>>())
}

fn strip_linux_url(url: &str) -> String {
    if !url.contains("__linux__") {
        return url.to_string();
    }
    let mut url_parts = url.split('/');
    let mut new_url = Vec::new();
    while let Some(part) = url_parts.next() {
        if part == "__linux__" {
            url_parts.next(); // Skip the next path element
        } else {
            new_url.push(part);
        }
    }
    new_url.join("/")
}

pub fn init_structure(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    let project_directory = project_directory.as_ref();
    create_library_structure(project_directory)?;
    create_gitignore(project_directory)?;
    Ok(())
}

fn create_library_structure(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    let lib_dir = project_directory.as_ref().join(LIBRARY_PATH);
    if lib_dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(project_directory.as_ref().join(LIBRARY_PATH))?;
    Ok(())
}

fn create_gitignore(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    let path = project_directory.as_ref().join(GITIGNORE_PATH);
    if path.exists() {
        return Ok(());
    }

    let content = format!("{LIBRARY_ROOT_DIR_NAME}\n");

    write(path, content)?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("Initialize error: {source}")]
#[non_exhaustive]
pub struct InitError {
    source: InitErrorKind,
}

#[derive(Debug, thiserror::Error)]
pub enum InitErrorKind {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("R command failed: {0}")]
    Command(std::io::Error),
    #[error("Failed to find repositories: {0}")]
    CommandFailed(String),
}

impl From<io::Error> for InitError {
    fn from(value: io::Error) -> Self {
        Self {
            source: InitErrorKind::Io(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        Repository, Version,
        cli::commands::init::{CONFIG_FILENAME, GITIGNORE_PATH, LIBRARY_PATH},
    };

    use super::{init, strip_linux_url};
    use tempfile::tempdir;
    use url::Url;

    #[test]
    fn test_init_content() {
        let project_directory = tempdir().unwrap();
        let r_version = Version::from_str("4.4.1").unwrap();
        let repositories = vec![
            Repository::new(
                "test1".to_string(),
                Url::parse("http://test1.com").unwrap(),
                true,
            ),
            Repository::new(
                "test2".to_string(),
                Url::parse("http://test2.com").unwrap(),
                false,
            ),
        ];
        let dependencies = vec!["dplyr".to_string()];
        init(
            &project_directory,
            &r_version.original,
            &repositories,
            &dependencies,
            false,
        )
        .unwrap();
        let dir = &project_directory.path();
        assert!(dir.join(LIBRARY_PATH).exists());
        assert!(dir.join(GITIGNORE_PATH).exists());
        assert!(dir.join(CONFIG_FILENAME).exists());
    }

    #[test]
    fn test_linux_url_strip() {
        let urls = [
            "https://packagemanager.posit.co/cran/latest",
            "https://packagemanager.posit.co/cran/__linux__/jammy/latest",
        ];
        let cleaned_urls = urls.iter().map(|u| strip_linux_url(u)).collect::<Vec<_>>();
        assert_eq!(cleaned_urls[0], cleaned_urls[1]);
    }
}
