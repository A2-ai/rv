use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use crate::consts::{GLOBAL_ACTIVATE_FILE_CONTENT, PROJECT_ACTIVATE_FILE_CONTENT};

// constant file name and function to provide the R code string to source the file
const ACTIVATE_FILE_NAME: &str = "rv/scripts/activate.R";
fn source_activate_file_string() -> String {
    format!(r#"source("{ACTIVATE_FILE_NAME}")"#)
}

pub fn activate(dir: impl AsRef<Path>) -> Result<(), ActivateError> {
    let dir = dir.as_ref();

    // ensure the directory is a directory and that it exists. If not, activation cannot occur
    if !dir.is_dir() {
        return Err(ActivateError {
            source: ActivateErrorKind::NotDir(dir.to_path_buf()),
        });
    }

    // read the .Rprofile, it the .Rprofile does not exist, create it. Then add the source command to the .Rprofile
    let (rprofile_content, rprofile_file) = read_file(&dir.join(".Rprofile"))?;
    if !rprofile_content.contains(&source_activate_file_string()) {
        activate_rprofile(rprofile_content, rprofile_file)?;
    }

    write_activate_file(dir)?;

    Ok(())
}

pub fn deactivate(dir: impl AsRef<Path>) -> Result<(), ActivateError> {
    let dir = dir.as_ref();
    let file_name = dir.join(".Rprofile");

    // if the .Rprofile does not exist, the directory is already "deactivated"
    if !file_name.exists() {
        return Ok(())
    }

    let (content, mut file) = read_file(file_name)?;
    
    // if the .Rprofile does not contain the activate source string, the directory is already "deactivated"
    if !content.contains(&source_activate_file_string()) {
        return Ok(());
    }

    // remove the activate source string
    let content = content.replace(&source_activate_file_string(), "");
    file.write_all(content.as_bytes())
        .map_err(|e| ActivateError {
            source: ActivateErrorKind::Io(e),
        })?;

    Ok(())
}

// Read a file, if the file does not exist, create it
// Return both the content and the File object for editing
// Need to read the content, not just add new line for deactivate
fn read_file(file_name: impl AsRef<Path>) -> Result<(String, File), ActivateError> {
    let file_name = file_name.as_ref();
    // if the file does not exist, create it and return the content as ""
    if !file_name.exists() {
        let file = File::create(file_name).map_err(|e| ActivateError {
            source: ActivateErrorKind::Io(e),
        })?;
        return Ok((String::new(), file));
    }
    let mut file = File::open(file_name).map_err(|e| ActivateError {
        source: ActivateErrorKind::Io(e),
    })?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|e| ActivateError {
            source: ActivateErrorKind::Io(e),
        })?;
    Ok((content, file))
}

fn activate_rprofile(rprofile_content: String, mut file: File) -> Result<(), ActivateError> {
    let content = format!(r#"{}\n{}"#, rprofile_content, source_activate_file_string());
    file.write_all(content.as_bytes())
        .map_err(|e| ActivateError {
            source: ActivateErrorKind::Io(e),
        })?;
    Ok(())
}

fn write_activate_file(dir: impl AsRef<Path>) -> Result<(), ActivateError> {
    let dir = dir.as_ref();

    // Determine if the content of the activate file is for global or project specific activation
    let content = match etcetera::home_dir()
        .map(|home| home == dir)
        .unwrap_or(false)
    {
        true => GLOBAL_ACTIVATE_FILE_CONTENT,
        false => PROJECT_ACTIVATE_FILE_CONTENT,
    };

    // read the file and determine if the content within the activate file matches
    // File may exist but needs upgrade if file changes with rv upgrade
    let (activate_content, mut file) = read_file(dir.join(ACTIVATE_FILE_NAME))?;
    if content == activate_content {
        return Ok(());
    }

    // Write the content of activate file
    file.write_all(content.as_bytes())
        .map_err(|e| ActivateError {
            source: ActivateErrorKind::Io(e),
        })?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("Activate error: {source}")]
#[non_exhaustive]
pub struct ActivateError {
    source: ActivateErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum ActivateErrorKind {
    #[error("{0} is not a directory")]
    NotDir(PathBuf),
    Io(std::io::Error),
}

