use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::LazyLock;

use crate::Version;
use crate::sync::{LinkError, LinkMode};
use regex::Regex;

static R_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap());

fn find_r_version(output: &str) -> Option<Version> {
    R_VERSION_RE
        .captures(output)
        .and_then(|c| c.get(0))
        .and_then(|m| Version::from_str(m.as_str()).ok())
}

pub trait RCmd: Send + Sync {
    /// Installs a package and returns the combined output of stdout and stderr
    fn install(
        &self,
        folder: impl AsRef<Path>,
        library: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<String, InstallError>;

    fn get_r_library(&self) -> Result<PathBuf, LibraryError>;

    fn version(&self) -> Result<Version, VersionError>;
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RCommandLine {
    /// specifies the path to the R executable on the system. None indicates using "R" on the $PATH
    pub r: Option<PathBuf>,
}

pub fn find_r_version_command(r_version: &Version) -> Result<RCommandLine, VersionError> {
    let mut found_r_vers = Vec::new();
    // Give preference to the R version on the path
    if let Ok(path_r) = (RCommandLine { r: None }).version() {
        if r_version.hazy_match(&path_r) {
            log::debug!("R {r_version} found on the path");
            return Ok(RCommandLine { r: None });
        }
        found_r_vers.push(path_r.original);
    }

    // For windows, R installed/managed by rig is has the extension .bat
    if cfg!(windows) {
        if let Ok(rig_r) = (RCommandLine {
            r: Some(PathBuf::from("R.bat")),
        })
        .version()
        {
            if r_version.hazy_match(&rig_r) {
                log::debug!("R {r_version} found on the path from `rig`");
                return Ok(RCommandLine {
                    r: Some(PathBuf::from("R.bat")),
                });
            }
            found_r_vers.push(rig_r.original);
        }
    }

    let opt_r = PathBuf::from("/opt/R");
    if opt_r.is_dir() {
        // look through subdirectories of '/opt/R' for R binaries and check if the binary is the correct version
        // returns an RCommandLine struct with the path to the executable if found
        for path in fs::read_dir(opt_r)
            .map_err(|e| VersionError {
                source: VersionErrorKind::Io(e),
            })?
            .filter_map(Result::ok)
            .map(|p| p.path().join("bin/R"))
            .filter(|p| p.exists())
        {
            if let Ok(ver) = (RCommandLine {
                r: Some(path.clone()),
            })
            .version()
            {
                if r_version.hazy_match(&ver) {
                    log::debug!(" R {r_version} found at {}", path.display());
                    return Ok(RCommandLine { r: Some(path) });
                }
                found_r_vers.push(ver.original);
            }
        }
    }

    if found_r_vers.is_empty() {
        Err(VersionError {
            source: VersionErrorKind::NoR,
        })
    } else {
        found_r_vers.sort();
        found_r_vers.dedup();
        Err(VersionError {
            source: VersionErrorKind::NotCompatible(
                r_version.original.to_string(),
                found_r_vers.join(", "),
            ),
        })
    }
}

impl RCmd for RCommandLine {
    fn install(
        &self,
        source_folder: impl AsRef<Path>,
        library: impl AsRef<Path>,
        destination: impl AsRef<Path>,
    ) -> Result<String, InstallError> {
        // Always delete destination if it exists first to avoid issues with incomplete installs
        // except if it's the same as the library. This happens for local packages
        if library.as_ref() != destination.as_ref() {
            if destination.as_ref().is_dir() {
                fs::remove_dir_all(destination.as_ref())
                    .map_err(|e| InstallError::from_fs_io(e, destination.as_ref()))?;
            }
            fs::create_dir_all(destination.as_ref())
                .map_err(|e| InstallError::from_fs_io(e, destination.as_ref()))?;
        }

        // We move the source to a temp dir since compilation might create a lot of artifacts that
        // we don't want to keep around in the cache once we're done
        // We symlink if possible except on Windows
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

        let mut command = Command::new(self.r.as_ref().unwrap_or(&PathBuf::from("R")));
        command
            .arg("CMD")
            .arg("INSTALL")
            // This is where it will be installed
            .arg(format!(
                "--library={}",
                destination.as_ref().to_string_lossy()
            ))
            .arg("--use-vanilla")
            .arg("--strip")
            .arg("--strip-lib")
            .arg(tmp_dir.path())
            // Override where R should look for deps
            .env("R_LIBS_SITE", &library)
            .env("R_LIBS_USER", &library)
            .env("_R_SHLIB_STRIP_", "true")
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
            // Always delete the destination is an error happend
            if destination.as_ref().is_dir() {
                // We ignore that error intentionally since we want to keep the one from CLI
                let _ = fs::remove_dir_all(destination.as_ref());
            }

            return Err(InstallError {
                source: InstallErrorKind::InstallationFailed(output),
            });
        }

        Ok(output)
    }

    fn get_r_library(&self) -> Result<PathBuf, LibraryError> {
        let output = Command::new(self.r.as_ref().unwrap_or(&PathBuf::from("R")))
            .arg("RHOME")
            .output()
            .map_err(|e| LibraryError {
                source: LibraryErrorKind::Io(e),
            })?;

        let stdout = std::str::from_utf8(if cfg!(windows) {
            &output.stderr
        } else {
            &output.stdout
        })
        .map_err(|e| LibraryError {
            source: LibraryErrorKind::Utf8(e),
        })?;

        let lib_path = PathBuf::from(stdout.trim()).join("library");

        if lib_path.is_dir() {
            Ok(lib_path)
        } else {
            Err(LibraryError {
                source: LibraryErrorKind::NotFound,
            })
        }
    }

    fn version(&self) -> Result<Version, VersionError> {
        let output = Command::new(self.r.as_ref().unwrap_or(&PathBuf::from("R")))
            .arg("--version")
            .output()
            .map_err(|e| VersionError {
                source: VersionErrorKind::Io(e),
            })?;

        // R.bat on Windows will write to stderr rather than stdout for some reasons
        let stdout = std::str::from_utf8(if cfg!(windows) {
            &output.stderr
        } else {
            &output.stdout
        })
        .map_err(|e| VersionError {
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
    #[error("R not found on system")]
    NoR,
    #[error(
        "Specified R version ({0}) does not match any available versions found on the system ({1})"
    )]
    NotCompatible(String, String),
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to get R version")]
#[non_exhaustive]
pub struct LibraryError {
    pub source: LibraryErrorKind,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum LibraryErrorKind {
    Io(#[from] std::io::Error),
    Utf8(#[from] std::str::Utf8Error),
    #[error("Library for current R not found")]
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
