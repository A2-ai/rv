use std::{
    io::Write,
    path::Path,
};

/// This function initializes a given directory to be a rv project. It does this by:
/// - Creating the directory if it does not exist
/// - Creating the library directory (<path/to/directory>/rv/library)
/// - Creating a .gitignore file within the rv subdirectory to ignore installed packages
pub fn init<P: AsRef<Path>>(project_directory: P) -> Result<(), InitError> {
    create_library_structure(&project_directory)?;
    write_gitignore(&project_directory)?;
    Ok(())
}

fn create_library_structure<P: AsRef<Path>>(project_directory: P) -> Result<(), InitError> {
    std::fs::create_dir_all(project_directory.as_ref().join("rv/library")).map_err(
        |e| InitError {
            source: InitErrorKind::Io(e),
        },
    )?;

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

    file.write_all(b"library/\nstaging/\n").map_err(|e| InitError {
        source: InitErrorKind::Io(e)
    })?;
    
    Ok(())
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
}

mod tests {
    use std::fs;

    use super::*;
    #[test]
    fn initialize_directory() {
        let project_directory = "./test-init";
        init(project_directory).unwrap();
        assert!(Path::new(&format!("{project_directory}/rv/library")).exists());
        assert!(Path::new(&format!("{project_directory}/rv/.gitignore")).exists());
        fs::remove_dir_all(project_directory).unwrap();
    }
}
