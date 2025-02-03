use std::{io::Write, path::Path};

const GITIGNORE_CONTENT: &str = "library/\nstaging/\n";
const GITIGNORE_PATH: &str = "rv/.gitignore";
const LIBRARY_PATH: &str = "rv/library";

pub fn init(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    let proj_dir = project_directory.as_ref();
    
    create_library_structure(proj_dir)?;
    create_gitignore(proj_dir)?;
    Ok(())
}

fn create_library_structure(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    std::fs::create_dir_all(project_directory.as_ref().join(LIBRARY_PATH)).map_err(|e| InitError {
        source: InitErrorKind::Io(e)
    })
}

fn create_gitignore(project_directory: impl AsRef<Path>) -> Result<(), InitError> {
    let path = project_directory.as_ref().join(GITIGNORE_PATH);
    if path.exists() {
        return Ok(());
    }

    let mut file = std::fs::File::create(path).map_err(|e| InitError {
        source: InitErrorKind::Io(e)
    })?;

    file.write_all(GITIGNORE_CONTENT.as_bytes()).map_err(|e| InitError {
        source: InitErrorKind::Io(e)
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
}

mod tests {
    use tempfile::tempdir;
    use super::init;

    #[test]
    fn test_init_content() {
        let project_directory = tempdir().unwrap();
        init(&project_directory).unwrap();
        let dir = &project_directory.into_path();
        assert!(dir.join("rv/library").exists());
        assert!(dir.join("rv/.gitignore").exists());
    }
}