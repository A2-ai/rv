use std::{fs::OpenOptions, io::{BufRead, BufReader, Write}, path::Path};

use fs_err::File;

pub fn init<P: AsRef<Path>>(project_directory: P) -> Result<(), InitError>{
    let project_directory = project_directory.as_ref().canonicalize().map_err(|e| InitError {
        source: InitErrorKind::Io(e),
    })?;
    write_gitignore(&project_directory)?;

    Ok(())
}

fn create_library_structure<P: AsRef<Path>>(project_directory: P) -> Result<(), InitError> {
    std::fs::create_dir_all(project_directory.as_ref().join("rv").join("library")).map_err(
        |e| InitError {
            source: InitErrorKind::Io(e),
        },
    )?;

    Ok(())
}

fn write_gitignore<P: AsRef<Path>>(project_directory: P) -> Result<(), InitError> {
    let project_directory = project_directory.as_ref();
    if !project_directory.exists() {
        create_library_structure(project_directory)?;
    }

    let file_path = project_directory.join("rv").join(".gitignore");
    let ignored_paths = ["library/", "staging/"];

    // check if gitignore file exists. If not, create one with "library/" and "staging/"
    if !file_path.exists() {
        let mut file = std::fs::File::create(&file_path)
            .map_err(|e| InitError {
                source: InitErrorKind::Io(e),
            })?;

        for p in ignored_paths {
            file.write_all(p.as_bytes()).map_err(|e| InitError {
                source: InitErrorKind::Io(e),
            })?;
        }

        return Ok(())
    };

    // if gitignore file does exist, write the missing "library/" and "staging" args
    let file = File::open(&file_path).map_err(|e| InitError {
        source: InitErrorKind::Io(e),
    })?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines().filter_map(Result::ok).collect::<Vec<_>>();

    let mut file = OpenOptions::new()
        .write(true)
        .append(true)
        .open(file_path)
        .map_err(|e| InitError {
            source: InitErrorKind::Io(e),
        })?;

    for p in ignored_paths {
        if !lines.contains(&p.to_string()) {
            file.write_all(p.as_bytes()).map_err(|e| InitError {
                source: InitErrorKind::Io(e),
            })?;
            lines.push(p.to_string());
        }
    }

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
