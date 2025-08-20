use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use std::{fs, thread};

use crate::fs::copy_folder;
use crate::sync::{LinkError, LinkMode};
use crate::{Cancellation, Version};
use regex::Regex;

static R_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap());

/// Since we create process group for our tasks, they won't be shutdown when we exit rv
/// so we do need to keep some references to them around so we can kill them manually.
/// We use the pid since we can't clone the handle.
pub static ACTIVE_R_PROCESS_IDS: LazyLock<Arc<Mutex<HashSet<u32>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashSet::new())));

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
        libraries: &[impl AsRef<Path>],
        destination: impl AsRef<Path>,
        cancellation: Arc<Cancellation>,
        env_vars: &HashMap<&str, &str>,
        configure_args: &[String],
    ) -> Result<String, InstallError>;

    fn get_r_library(&self) -> Result<PathBuf, LibraryError>;

    fn version(&self) -> Result<Version, VersionError>;
}

/// By default, doing ctrl+c on rv will kill it as well as all its child process.
/// To allow graceful shutdown, we create a process group in Unix and the equivalent on Windows
/// so we can control _how_ they get killed, and allow for a soft cancellation (eg we let
/// ongoing tasks finish but stop enqueuing/processing new ones.
fn spawn_isolated_r_command(r_cmd: &RCommandLine) -> Command {
    let mut command = Command::new(r_cmd.effective_r_command());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }

    command
}

#[cfg(feature = "cli")]
pub fn kill_all_r_processes() {
    let process_ids = ACTIVE_R_PROCESS_IDS.lock().unwrap();

    for pid in process_ids.iter() {
        #[cfg(unix)]
        {
            unsafe {
                libc::kill((*pid) as i32, libc::SIGTERM);
            }
        }

        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .arg("/PID")
                .arg(pid.to_string())
                .arg("/F")
                .output();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RCommandLine {
    /// specifies the path to the R executable on the system. None indicates using "R" on the $PATH
    pub r: Option<PathBuf>,
}

pub fn find_r_version_command(r_version: &Version) -> Result<RCommandLine, VersionError> {
    // TODO: increase test coverage for this function

    let mut found_r_vers = Vec::new();
    // Give preference to the R version on the path
    if let Ok(path_r) = (RCommandLine { r: None }).version() {
        if r_version.hazy_match(&path_r) {
            log::debug!("R found on the path: {path_r}");
            return Ok(RCommandLine { r: None });
        }
        found_r_vers.push(path_r.original);
    }

    // Include matching rig-formatted R on the path if it exists on macOS
    // e.g. R-<major>.<minor>-<arch>
    if cfg!(target_os = "macos") {
        let info = os_info::get();
        let major_minor = r_version.major_minor();
        if let Some(arch) = info.architecture() {
            let rig_r_bin_path =
                PathBuf::from(format!("R-{}.{}-{}", major_minor[0], major_minor[1], arch));

            let rig_r_cmd = RCommandLine {
                r: Some(rig_r_bin_path),
            };

            if let Ok(path_rig_r) = rig_r_cmd.version() {
                if r_version.hazy_match(&path_rig_r) {
                    log::debug!(
                        "R found on the path via rig pattern: {}",
                        rig_r_cmd.clone().r.unwrap().display()
                    );
                    return Ok(rig_r_cmd);
                }
                found_r_vers.push(path_rig_r.original);
            }
        }
    }

    // For windows, R installed/managed by rig is has the extension .bat
    if cfg!(target_os = "windows") {
        let rig_r_cmd = RCommandLine {
            r: Some(PathBuf::from("R.bat")),
        };

        if let Ok(rig_r) = rig_r_cmd.version() {
            if r_version.hazy_match(&rig_r) {
                log::debug!(
                    "R found on the path from `rig`: {}",
                    rig_r_cmd.clone().r.unwrap().display()
                );
                return Ok(rig_r_cmd);
            }
            found_r_vers.push(rig_r.original);
        }
    }

    if cfg!(target_os = "linux") {
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
                let r_cmd = RCommandLine {
                    r: Some(path.clone()),
                };
                if let Ok(ver) = r_cmd.version() {
                    if r_version.hazy_match(&ver) {
                        log::debug!("R found in /opt/R: {}", r_cmd.clone().r.unwrap().display());
                        return Ok(r_cmd);
                    }
                    found_r_vers.push(ver.original);
                }
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

impl RCommandLine {
    fn effective_r_command(&self) -> PathBuf {
        if let Some(ref r_path) = self.r {
            return r_path.clone();
        }

        #[cfg(windows)]
        {
            // On Windows, check if R.bat exists in PATH, otherwise default to R
            if which::which("R.bat").is_ok() {
                PathBuf::from("R.bat")
            } else {
                PathBuf::from("R")
            }
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("R")
        }
    }
}

impl RCmd for RCommandLine {
    fn install(
        &self,
        source_folder: impl AsRef<Path>,
        libraries: &[impl AsRef<Path>],
        destination: impl AsRef<Path>,
        cancellation: Arc<Cancellation>,
        env_vars: &HashMap<&str, &str>,
        configure_args: &[String],
    ) -> Result<String, InstallError> {
        let destination = destination.as_ref();
        // We create a temp build dir so we only remove an existing destination if we have something we can replace it with
        let build_dir = tempfile::tempdir().map_err(|e| InstallError {
            source: InstallErrorKind::TempDir(e),
        })?;

        // We move the source to a temp dir since compilation might create a lot of artifacts that
        // we don't want to keep around in the cache once we're done
        // We symlink if possible except on Windows
        let src_backup_dir = tempfile::tempdir().map_err(|e| InstallError {
            source: InstallErrorKind::TempDir(e),
        })?;
        LinkMode::Copy
            .link_files("tmp_build", &source_folder, src_backup_dir.path())
            .map_err(|e| InstallError {
                source: InstallErrorKind::LinkError(e),
            })?;

        let canonicalized_libraries = libraries
            .iter()
            .map(|lib| lib.as_ref().canonicalize())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| InstallError::from_fs_io(e, destination))?;

        // combine them to the single string path that R wants, specifically:
        //  colon-separated on Unix-alike systems and semicolon-separated on Windows.
        let library_paths = if cfg!(windows) {
            canonicalized_libraries
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join(";")
        } else {
            canonicalized_libraries
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join(":")
        };

        let (recv, send) = std::io::pipe().map_err(|e| InstallError::from_fs_io(e, destination))?;
        let mut command = spawn_isolated_r_command(self);
        command
            .arg("CMD")
            .arg("INSTALL")
            // This is where it will be installed
            .arg(format!(
                "--library={}",
                build_dir.as_ref().to_string_lossy()
            ))
            .arg("--use-vanilla")
            .arg("--strip")
            .arg("--strip-lib");

        // Add configure args (Unix only - Windows R CMD INSTALL doesn't support --configure-args)
        // configure-args are unix only and should be a single string per:
        // https://cran.r-project.org/doc/manuals/r-devel/R-exts.html#Configure-example-1
        #[cfg(unix)]
        if !configure_args.is_empty() {
            #[cfg(unix)]
            if !configure_args.is_empty() {
                let combined_args = configure_args.join(" ");
                log::debug!(
                    "Adding configure args for {}: {}",
                    source_folder.as_ref().display(),
                    combined_args
                );
                command.arg(format!("--configure-args='{}'", combined_args));
            }
        }
        command
            .arg(src_backup_dir.path())
            // Override where R should look for deps
            .env("R_LIBS_SITE", &library_paths)
            .env("R_LIBS_USER", &library_paths)
            .env("_R_SHLIB_STRIP_", "true")
            .stdout(
                send.try_clone()
                    .map_err(|e| InstallError::from_fs_io(e, destination))?,
            )
            .stderr(send)
            .envs(env_vars);
        log::debug!(
            "Compiling {} with env vars: {}",
            source_folder.as_ref().display(),
            command
                .get_envs()
                .into_iter()
                .map(|(k, v)| format!(
                    "{}={}",
                    k.to_string_lossy(),
                    v.unwrap_or_default().to_string_lossy()
                ))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut handle = command.spawn().map_err(|e| InstallError {
            source: InstallErrorKind::Command(e),
        })?;

        let pid = handle.id();

        {
            let mut process_ids = ACTIVE_R_PROCESS_IDS.lock().unwrap();
            process_ids.insert(pid);
        }

        // deadlock otherwise according to os_pipe docs
        drop(command);

        // Read output in a separate thread to avoid blocking on pipe buffers
        let output_handle = {
            let mut recv = recv;
            thread::spawn(move || {
                let mut output = String::new();
                let _ = recv.read_to_string(&mut output);
                output
            })
        };

        let cleanup = |output| {
            {
                let mut process_ids = ACTIVE_R_PROCESS_IDS.lock().unwrap();
                process_ids.remove(&pid);
            }

            if destination.is_dir() {
                // We ignore that error intentionally since we want to keep the one from CLI
                if let Err(e) = fs::remove_dir_all(destination) {
                    log::error!(
                        "Failed to remove directory `{}` after R CMD INSTALL failed: {e}. Delete this folder manually",
                        destination.display()
                    );
                }
            }
            return Err(InstallError {
                source: InstallErrorKind::InstallationFailed(output),
            });
        };

        // Poll for completion or cancellation
        loop {
            // Did the command finish?
            match handle.try_wait() {
                Ok(Some(status)) => {
                    {
                        let mut process_ids = ACTIVE_R_PROCESS_IDS.lock().unwrap();
                        process_ids.remove(&pid);
                    }
                    // Process finished, get the output from the reading thread
                    let output = output_handle.join().unwrap();

                    if !status.success() {
                        return cleanup(output);
                    }

                    // If it's a success, copy the build tmp dir to the actual destination
                    // we don't move the folder since the tmp dir might be in another drive/format
                    // than the cache dir
                    fs::create_dir_all(destination)
                        .map_err(|e| InstallError::from_fs_io(e, destination))?;
                    copy_folder(build_dir.as_ref(), destination)
                        .map_err(|e| InstallError::from_fs_io(e, destination))?;

                    return Ok(output);
                }
                Ok(None) => {
                    // Process still running, check for cancellation
                    if cancellation.is_soft_cancellation() {
                        // On soft cancellation, let R finish naturally
                        // On hard cancellation, rv will kill
                        let status = handle.wait().unwrap();
                        let output = output_handle.join().unwrap();

                        if !status.success() {
                            return cleanup(output);
                        }

                        return Ok(output);
                    }

                    // Sleep briefly to avoid busy waiting
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(InstallError {
                        source: InstallErrorKind::Command(e),
                    });
                }
            }
        }
    }

    fn get_r_library(&self) -> Result<PathBuf, LibraryError> {
        let output = Command::new(self.effective_r_command())
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
        let output = Command::new(self.effective_r_command())
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
    #[error("Installation cancelled by user")]
    Cancelled,
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
