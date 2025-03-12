use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    process::Command,
};

use crate::Repository;

const GITIGNORE_CONTENT: &str = "library/\nstaging/\n";
const GITIGNORE_PATH: &str = "rv/.gitignore";
const LIBRARY_PATH: &str = "rv/library";
const CONFIG_FILENAME: &str = "rproject.toml";

const INITIAL_CONFIG: &str = r#"[project]
name = "%project_name%"
r_version = "%r_version%"

# any CRAN-type repository, order matters. Additional ability to force source package installation
# Example: {alias = "CRAN", url = "https://cran.r-project.org", force_source = true}
repositories = [
%repositories%
]

# package to install and any specifications. Any descriptive dependency can have the `install_suggestions` specification
# Examples:
    # "dplyr",
    # {name = "dplyr", repository = "CRAN", force_source = true},
    # {name = "dplyr", git = "https://github.com/tidyverse/dplyr.git", tag = "v1.1.4"},
    # {name = "dplyr", path = "/path/to/local/dplyr"},
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
) -> Result<(), InitError> {
    let proj_dir = project_directory.as_ref();
    create_library_structure(proj_dir)?;
    create_gitignore(proj_dir)?;
    let project_name = proj_dir
        .canonicalize()
        .map_err(|e| InitError {
            source: InitErrorKind::Io(e),
        })?
        .iter()
        .last()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or("my rv project".to_string());

    let config = render_config(&project_name, &r_version, &repositories, dependencies);

    let mut file = File::create(proj_dir.join(CONFIG_FILENAME)).map_err(|e| InitError {
        source: InitErrorKind::Io(e),
    })?;

    file.write_all(config.as_bytes()).map_err(|e| InitError {
        source: InitErrorKind::Io(e),
    })?;
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

    let (mut reader, writer) = os_pipe::pipe().map_err(|e| InitError {
        source: InitErrorKind::Command(e),
    })?;
    let writer_clone = writer.try_clone().map_err(|e| InitError {
        source: InitErrorKind::Command(e),
    })?;

    let mut command = Command::new("Rscript");
    command
        .arg("-e")
        .arg(r_code)
        .stdout(writer)
        .stderr(writer_clone);

    let mut handle = command.spawn().map_err(|e| InitError {
        source: InitErrorKind::Command(e),
    })?;

    drop(command);

    let mut output = String::new();
    reader.read_to_string(&mut output).unwrap();
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
            Some(Repository::new(alias, url, false))
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

pub fn create_library_structure(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    let lib_dir = project_directory.as_ref().join(LIBRARY_PATH);
    if lib_dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(project_directory.as_ref().join(LIBRARY_PATH)).map_err(|e| InitError {
        source: InitErrorKind::Io(e),
    })
}

pub fn create_gitignore(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    let path = project_directory.as_ref().join(GITIGNORE_PATH);
    if path.exists() {
        return Ok(());
    }

    let mut file = std::fs::File::create(path).map_err(|e| InitError {
        source: InitErrorKind::Io(e),
    })?;

    file.write_all(GITIGNORE_CONTENT.as_bytes())
        .map_err(|e| InitError {
            source: InitErrorKind::Io(e),
        })
}

#[derive(Debug, thiserror::Error)]
#[error("Lockfile error: {source}")]
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        cli::commands::init::{CONFIG_FILENAME, GITIGNORE_PATH, LIBRARY_PATH},
        Repository, Version,
    };

    use super::{init, strip_linux_url};
    use tempfile::tempdir;

    #[test]
    fn test_init_content() {
        let project_directory = tempdir().unwrap();
        let r_version = Version::from_str("4.4.1").unwrap();
        let repositories = vec![
            Repository::new("test1".to_string(), "this is test1".to_string(), true),
            Repository::new("test2".to_string(), "this is test2".to_string(), false),
        ];
        let dependencies = vec!["dplyr".to_string()];
        init(
            &project_directory,
            &r_version.original,
            &repositories,
            &dependencies,
        )
        .unwrap();
        let dir = &project_directory.into_path();
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
