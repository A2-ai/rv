use std::{
    io,
    path::{Path, PathBuf},
};

use fs_err::{read_to_string, write};

use crate::consts::{ACTIVATE_FILE_TEMPLATE, RVR_FILE_CONTENT};

// constant file name and function to provide the R code string to source the file
const ACTIVATE_FILE_NAME: &str = "rv/scripts/activate.R";
const RVR_FILE_NAME: &str = "rv/scripts/rvr.R";

pub fn activate(dir: impl AsRef<Path>, no_r_environment: bool) -> Result<(), ActivateError> {
    let dir = dir.as_ref();

    // ensure the directory is a directory and that it exists. If not, activation cannot occur
    if !dir.is_dir() {
        return Err(ActivateError {
            source: ActivateErrorKind::NotDir(dir.to_path_buf()),
        });
    }

    let is_home = is_home_dir(&dir.canonicalize()?);
    let (activate_source_path, rvr_source_path) = scripts_as_paths(is_home);

    write_activate_file(dir, is_home)?;
    add_rprofile_source_call(dir, activate_source_path)?;
    write_rvr_file(dir)?;
    if !no_r_environment {
        add_rprofile_source_call(dir, rvr_source_path)?;
    }
    Ok(())
}

fn add_rprofile_source_call(
    dir: impl AsRef<Path>,
    source_file: impl AsRef<Path>,
) -> Result<(), io::Error> {
    let path = dir.as_ref().join(".Rprofile");
    let source_file = source_file.as_ref();

    let content = if path.exists() {
        read_to_string(&path)?
    } else {
        String::new()
    };

    if content.contains(&*source_file.to_string_lossy()) {
        return Ok(());
    }
    let source_str = format!(r#"source("{}")"#, source_file.display());
    let new_content = format!("{}\n{}", source_str, content);
    write(path, new_content)?;

    Ok(())
}

pub fn deactivate(dir: impl AsRef<Path>) -> Result<(), ActivateError> {
    let dir = dir.as_ref();
    let rprofile_path = dir.join(".Rprofile");

    if !rprofile_path.exists() {
        return Ok(());
    }

    let is_home = is_home_dir(&dir.canonicalize()?);
    let (activate_path, rvr_path) = scripts_as_paths(is_home);

    let content = read_to_string(&rprofile_path)?;
    let new_content = content
        .lines()
        .filter(|line| line != &format!(r#"source("{}")"#, activate_path.display()))
        .filter(|line| line != &format!(r#"source("{}")"#, rvr_path.display()))
        .collect::<Vec<_>>()
        .join("\n");

    write(&rprofile_path, new_content)?;

    Ok(())
}

fn is_home_dir(dir: impl AsRef<Path>) -> bool {
    etcetera::home_dir()
        .map(|home| home == dir.as_ref())
        .unwrap_or(false)
}

fn scripts_as_paths(is_home: bool) -> (PathBuf, PathBuf) {
    if is_home {
        let home = PathBuf::from("~");
        (home.join(ACTIVATE_FILE_NAME), home.join(RVR_FILE_NAME))
    } else {
        (ACTIVATE_FILE_NAME.into(), RVR_FILE_NAME.into())
    }
}

fn write_activate_file(dir: impl AsRef<Path>, is_home: bool) -> Result<(), ActivateError> {
    let template = ACTIVATE_FILE_TEMPLATE.to_string();
    let global_wd_content = if is_home
    {
        r#"
        owd <- getwd()
        setwd("~")
        on.exit({
            setwd(owd)
        })"#
    } else {
        ""
    };
    let rv_command = if cfg!(windows) { "rv.exe" } else { "rv" };
    let content = template
        .replace("%rv command%", rv_command)
        .replace("%global wd content%", global_wd_content);
    // read the file and determine if the content within the activate file matches
    // File may exist but needs upgrade if file changes with rv upgrade
    let activate_file_name = dir.as_ref().join(ACTIVATE_FILE_NAME);
    if let Some(parent) = activate_file_name.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let activate_content = read_to_string(&activate_file_name).unwrap_or_default();
    if content == activate_content {
        return Ok(());
    }

    // Write the content of activate file
    write(&activate_file_name, content)?;
    Ok(())
}

fn write_rvr_file(project_directory: impl AsRef<Path>) -> Result<(), ActivateError> {
    let path = project_directory.as_ref().join(RVR_FILE_NAME);
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write(path, RVR_FILE_CONTENT)?;
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

impl From<io::Error> for ActivateError {
    fn from(value: io::Error) -> Self {
        Self {
            source: ActivateErrorKind::Io(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::activate::RVR_FILE_NAME;

    use super::{activate, ACTIVATE_FILE_NAME};

    #[test]
    fn test_activation() {
        let tmp_dir = tempfile::tempdir().unwrap();
        activate(&tmp_dir, false).unwrap();
        assert!(tmp_dir.path().join(ACTIVATE_FILE_NAME).exists());
        assert!(tmp_dir.path().join(RVR_FILE_NAME).exists());
        assert!(tmp_dir.path().join(".Rprofile").exists());
    }
}
