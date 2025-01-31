use std::{io::Write, path::Path};

use crate::{Repository, Version};

const GITIGNORE_CONTENT: &str = "library/\nstaging/\n";
const GITIGNORE_PATH: &str = "rv/.gitignore";
const LIBRARY_PATH: &str = "rv/library";

pub fn init(project_directory: impl AsRef<Path>, r_version: Version, repositories: Vec<Repository>) -> Result<(), InitError> {
    let proj_dir = project_directory.as_ref();

    create_library_structure(proj_dir)?;
    create_gitignore(proj_dir)?;
    Ok(())
}

pub fn find_r_repositories() -> Result<Vec<Repository>, InitError> {
    let r_code = r#"
    repos <- getOption("repos")
    cat(paste(names(repos), repos, sep = "\t", collapse = "\n"))
    "#;

    let output = std::process::Command::new("Rscript")
        .arg("-e")
        .arg(r_code)
        .output()
        .map_err(|e| InitError {
            source: InitErrorKind::Io(e),
        })?;
    if !output.status.success() {
        return Err(InitError {
            source: InitErrorKind::CommandFailed {
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            },
        });
    };

    Ok(String::from_utf8_lossy(&output.stdout)
        .as_ref()
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            Some(Repository::new(
                parts.next()?.to_string(),
                parts.next()?.to_string(),
                false,
            ))
        })
        .collect::<Vec<_>>())
}

fn create_library_structure(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    std::fs::create_dir_all(project_directory.as_ref().join(LIBRARY_PATH)).map_err(|e| InitError {
        source: InitErrorKind::Io(e),
    })
}

fn create_gitignore(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
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
    #[error("R command failed with status {status}. stderr: {stderr}")]
    CommandFailed {
        status: std::process::ExitStatus,
        stderr: String,
    },
}

mod tests {
    use std::str::FromStr;

    use crate::{Repository, Version};

    use super::init;
    use tempfile::tempdir;

    #[test]
    fn test_init_content() {
        let project_directory = tempdir().unwrap();
        let r_version = Version::from_str("4.4.1").unwrap();
        let repositories = vec![
            Repository::new("test1".to_string(), "this is test1".to_string(), true),
            Repository::new("test2".to_string(), "this is test2".to_string(), false),
        ];
        init(&project_directory, r_version, repositories).unwrap();
        let dir = &project_directory.into_path();
        assert!(dir.join("rv/library").exists());
        assert!(dir.join("rv/.gitignore").exists());
    }
}
