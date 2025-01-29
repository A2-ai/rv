use std::{io::Write, path::Path};

use crate::{Repository, Version};

/// This function initializes a given directory to be a rv project. It does this by:
/// - Creating the directory if it does not exist
/// - Creating the library directory (<path/to/directory>/rv/library)
/// - Creating a .gitignore file within the rv subdirectory to ignore installed packages
pub fn init<P: AsRef<Path>>(
    project_directory: P,
    r_version: Version,
    repository: Vec<Repository>,
) -> Result<(), InitError> {
    create_library_structure(&project_directory)?;
    write_gitignore(&project_directory)?;
    Ok(())
}

fn create_library_structure<P: AsRef<Path>>(project_directory: P) -> Result<(), InitError> {
    std::fs::create_dir_all(project_directory.as_ref().join("rv/library")).map_err(|e| {
        InitError {
            source: InitErrorKind::Io(e),
        }
    })?;

    Ok(())
}

fn write_gitignore<P: AsRef<Path>>(project_directory: P) -> Result<(), InitError> {
    let file_path = project_directory.as_ref().join("rv/.gitignore");
    if file_path.as_path().exists() {
        return Ok(());
    };

    let mut file = std::fs::File::create(file_path).map_err(|e| InitError {
        source: InitErrorKind::Io(e),
    })?;

    file.write_all(b"library/\nstaging/\n")
        .map_err(|e| InitError {
            source: InitErrorKind::Io(e),
        })?;

    Ok(())
}

pub fn determine_repository_from_r() -> Result<Vec<Repository>, InitError> {
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

#[derive(Debug, thiserror::Error)]
#[error("Lockfile error: {source}")]
#[non_exhaustive]
pub struct InitError {
    pub source: InitErrorKind,
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
    use std::{fs, str::FromStr};

    use super::*;
    #[test]
    fn initialize_directory() {
        let project_directory = "./test-init";
        let r_version = Version::from_str("4.4.1").unwrap();
        let mut repository = Vec::new();
        repository.push(Repository::new(
            "RSPM".to_string(),
            "https://packagemanager.posit.co/cran/2024-10-06".to_string(),
            false,
        ));
        let _ = init(project_directory, r_version, repository);
        assert!(Path::new(&format!("{project_directory}/rv/library")).exists());
        assert!(Path::new(&format!("{project_directory}/rv/.gitignore")).exists());
        fs::remove_dir_all(project_directory).unwrap();
    }

    // #[test]
    // fn test_repository() {
    //     println!("{:#?}", determine_repository_from_r().unwrap());
    // }
}
