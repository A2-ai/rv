use std::path::{Path, PathBuf};

use fs_err::{read_to_string, write};

use crate::consts::ACTIVATE_FILE_TEMPLATE;

// constant file name and function to provide the R code string to source the file
const ACTIVATE_FILE_NAME: &str = "rv/scripts/activate.R";
fn activation_string() -> String {
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

    write_activate_file(dir)?;

    let rprofile_path = dir.join(".Rprofile");
    if !rprofile_path.exists() {
        write(&rprofile_path, format!("{}\n", activation_string())).map_err(|e| ActivateError {
            source: ActivateErrorKind::Io(e),
        })?;
    };

    let content = read_to_string(&rprofile_path).map_err(|e| ActivateError {
        source: ActivateErrorKind::Io(e),
    })?;

    if content.contains(&activation_string()) {
        return Ok(());
    }

    let new_content = format!("{}\n{}", activation_string(), content);
    write(rprofile_path, new_content).map_err(|e| ActivateError {
        source: ActivateErrorKind::Io(e),
    })?;

    Ok(())
}

pub fn deactivate(dir: impl AsRef<Path>) -> Result<(), ActivateError> {
    let dir = dir.as_ref();
    let rprofile_path = dir.join(".Rprofile");

    if !rprofile_path.exists() {
        return Ok(());
    }

    let content = read_to_string(&rprofile_path).map_err(|e| ActivateError {
        source: ActivateErrorKind::Io(e),
    })?;

    let new_content = content
        .lines()
        .filter(|line| line != &activation_string())
        .collect::<Vec<_>>()
        .join("\n");

    write(&rprofile_path, new_content).map_err(|e| ActivateError {
        source: ActivateErrorKind::Io(e),
    })?;

    Ok(())
}

fn write_activate_file(dir: impl AsRef<Path>) -> Result<(), ActivateError> {
    let dir = dir.as_ref().canonicalize().map_err(|e| ActivateError {
        source: ActivateErrorKind::Io(e),
    })?;

    let template = ACTIVATE_FILE_TEMPLATE.to_string();
    let global_wd_content = if etcetera::home_dir()
        .map(|home| home == dir)
        .unwrap_or(false)
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
    let activate_file_name = &dir.join(ACTIVATE_FILE_NAME);
    if let Some(parent) = activate_file_name.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ActivateError {
            source: ActivateErrorKind::Io(e),
        })?;
    }
    let activate_content = read_to_string(activate_file_name).unwrap_or_default();
    if content == activate_content {
        return Ok(());
    }

    // Write the content of activate file
    write(activate_file_name, content).map_err(|e| ActivateError {
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

#[cfg(test)]
mod tests {
    use super::{activate, ACTIVATE_FILE_NAME};

    #[test]
    fn test_activation() {
        let tmp_dir = tempfile::tempdir().unwrap();
        activate(&tmp_dir).unwrap();
        assert!(tmp_dir.path().join(ACTIVATE_FILE_NAME).exists());
        assert!(tmp_dir.path().join(".Rprofile").exists());
    }
}
