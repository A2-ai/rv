use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::LazyLock;

use crate::link::{LinkError, LinkMode};
use crate::version::Version;
use regex::Regex;

static R_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap());

fn find_r_version(output: &str) -> Option<Version> {
    R_VERSION_RE
        .captures(output)
        .and_then(|c| c.get(0))
        .and_then(|m| Version::from_str(m.as_str()).ok())
}

pub trait RCmd {
    /// Installs a package and returns the combined output of stdout and stderr
    fn install(
        &self,
        folder: impl AsRef<Path>,
        library: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<String, InstallError>;

    fn check(
        &self,
        file_path: &Path,
        result_path: &Path,
        args: Vec<&str>,
        env_var: Vec<(&str, &str)>,
    ) -> Result<(), std::io::Error>;

    fn build(
        &self,
        file_path: &Path,
        library: &Path,
        output_path: &Path,
        args: Vec<&str>,
        env_var: Vec<(&str, &str)>,
    ) -> Result<(), std::io::Error>;

    fn version(&self) -> Result<Version, VersionError>;
}

pub struct RCommandLine;

impl RCmd for RCommandLine {
    fn install(
        &self,
        source_folder: impl AsRef<Path>,
        library: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<String, InstallError> {
        // Always delete destination if it exists first to avoid issues with incomplete installs
        if destination.as_ref().is_dir() {
            fs::remove_dir_all(destination.as_ref())
                .map_err(|e| InstallError::from_fs_io(e, destination.as_ref()))?;
        }
        fs::create_dir_all(destination.as_ref())
            .map_err(|e| InstallError::from_fs_io(e, destination.as_ref()))?;

        // We move the source to a temp dir since compilation might create a lot of artifacts that
        // we don't want to keep around in the cache once we're done
        // Since it's right next to each other, we symlink if possible except on Windows
        let tmp_dir = tempfile::tempdir().map_err(|e| InstallError {
            source: InstallErrorKind::TempDir(e),
        })?;
        let link = LinkMode::symlink_if_possible();
        link.link_files("tmp_build", source_folder, tmp_dir.path())
            .map_err(|e| InstallError {
                source: InstallErrorKind::LinkError(e),
            })?;

        let (mut reader, writer) = os_pipe::pipe().map_err(|e| InstallError {
            source: InstallErrorKind::Command(e),
        })?;
        let writer_clone = writer.try_clone().map_err(|e| InstallError {
            source: InstallErrorKind::Command(e),
        })?;

        let library = library.as_ref().canonicalize().map_err(|e| InstallError {
            source: InstallErrorKind::Command(e),
        })?;

        let mut command = Command::new("R");
        command
            .arg("CMD")
            .arg("INSTALL")
            // This is where it will be installed
            .arg(format!(
                "--library={}",
                destination.as_ref().to_string_lossy()
            ))
            .arg("--use-vanilla")
            .arg(tmp_dir.path())
            // Override where R should look for deps
            .env("R_LIBS_SITE", &library)
            .env("R_LIBS_USER", &library)
            .stdout(writer)
            .stderr(writer_clone);

        let mut handle = command.spawn().map_err(|e| InstallError {
            source: InstallErrorKind::Command(e),
        })?;

        // deadlock otherwise according to os_pipe docs
        drop(command);

        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap();
        let status = handle.wait().unwrap();

        if !status.success() {
            return Err(InstallError {
                source: InstallErrorKind::InstallationFailed(output),
            });
        }

        Ok(output)
    }

    fn check(
        &self,
        _file_path: &Path,
        _result_path: &Path,
        _args: Vec<&str>,
        _env_var: Vec<(&str, &str)>,
    ) -> Result<(), std::io::Error> {
        todo!()
    }

    fn build(
        &self,
        _file_path: &Path,
        _library: &Path,
        _output_path: &Path,
        _args: Vec<&str>,
        _env_var: Vec<(&str, &str)>,
    ) -> Result<(), std::io::Error> {
        todo!()
    }

    fn version(&self) -> Result<Version, VersionError> {
        let output = Command::new("R")
            .arg("--version")
            .output()
            .map_err(|e| VersionError {
                source: VersionErrorKind::Io(e),
            })?;
        let stdout = std::str::from_utf8(&output.stdout).map_err(|e| VersionError {
            source: VersionErrorKind::Utf8(e),
        })?;
        if let Some(v) = find_r_version(stdout) {
            Ok(v)
        } else {
            Err(VersionError {
                source: VersionErrorKind::NotFound,
            })
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
#[non_exhaustive]
pub struct InstallError {
    pub source: InstallErrorKind,
}

impl InstallError {
    pub fn from_fs_io(error: std::io::Error, path: &Path) -> Self {
        Self {
            source: InstallErrorKind::File {
                error,
                path: path.to_path_buf(),
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallErrorKind {
    #[error("IO error: {error} ({path})")]
    File {
        error: std::io::Error,
        path: PathBuf,
    },
    #[error(transparent)]
    LinkError(LinkError),
    #[error("Failed to create or copy files to temp directory: {0}")]
    TempDir(std::io::Error),
    #[error("Command failed: {0}")]
    Command(std::io::Error),
    #[error(transparent)]
    Utf8(#[from] std::str::Utf8Error),
    #[error("Installation failed: {0}")]
    InstallationFailed(String),
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to get R version")]
#[non_exhaustive]
pub struct VersionError {
    pub source: VersionErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum VersionErrorKind {
    Io(#[from] std::io::Error),
    Utf8(#[from] std::str::Utf8Error),
    #[error("Version not found in R --version output")]
    NotFound,
}

#[allow(unused_imports, unused_variables)]
mod tests {
    use super::*;

    #[test]
    fn can_read_r_version() {
        let r_response = r#"/
R version 4.4.1 (2024-06-14) -- "Race for Your Life"
Copyright (C) 2024 The R Foundation for Statistical Computing
Platform: x86_64-pc-linux-gnu

R is free software and comes with ABSOLUTELY NO WARRANTY.
You are welcome to redistribute it under the terms of the
GNU General Public License versions 2 or 3.
For more information about these matters see
https://www.gnu.org/licenses/."#;
        assert_eq!(
            find_r_version(r_response).unwrap(),
            "4.4.1".parse::<Version>().unwrap()
        )
    }

    #[test]
    fn r_not_found() {
        let r_response = r#"/
Command 'R' is available in '/usr/local/bin/R'
The command could not be located because '/usr/local/bin' is not included in the PATH environment variable.
R: command not found"#;
        assert!(find_r_version(r_response).is_none());
    }
}
