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
use crate::r_finder::RInstall;
use crate::sync::{LinkError, LinkMode};
use crate::{Cancellation, Version};
use regex::Regex;

static R_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap());

fn is_r_devel(output: &str) -> bool {
    output.contains("R Under development")
}

fn find_r_version(output: &str) -> Option<Version> {
    R_VERSION_RE
        .find(output)
        .and_then(|m| Version::from_str(m.as_str()).ok())
}

/// Since we create process group for our tasks, they won't be shutdown when we exit rv
/// so we do need to keep some references to them around so we can kill them manually.
/// We use the pid since we can't clone the handle.
pub static ACTIVE_R_PROCESS_IDS: LazyLock<Arc<Mutex<HashSet<u32>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(HashSet::new())));

pub trait RCmd: Send + Sync {
    /// Installs a package and returns the combined output of stdout and stderr
    #[allow(clippy::too_many_arguments)]
    fn install(
        &self,
        folder: impl AsRef<Path>,
        sub_folder: Option<impl AsRef<Path>>,
        libraries: &[impl AsRef<Path>],
        destination: impl AsRef<Path>,
        cancellation: Arc<Cancellation>,
        env_vars: &HashMap<&str, &str>,
        configure_args: &[String],
        strip: bool,
    ) -> Result<String, RCmdError>;

    /// Runs `R CMD build` on a source directory and returns the path to the resulting tarball.
    fn build(
        &self,
        source_dir: impl AsRef<Path>,
        output_dir: impl AsRef<Path>,
        libraries: &[impl AsRef<Path>],
        cancellation: Arc<Cancellation>,
        env_vars: &HashMap<&str, &str>,
    ) -> Result<PathBuf, RCmdError>;

    fn get_r_library(&self) -> Result<PathBuf, LibraryError>;

    fn version(&self) -> Result<Option<Version>, VersionError>;
}

/// Canonicalize library paths and join them into R's expected format
/// (colon-separated on Unix, semicolon-separated on Windows).
fn r_library_paths(libraries: &[impl AsRef<Path>]) -> Result<String, std::io::Error> {
    let canonicalized = libraries
        .iter()
        .map(|lib| lib.as_ref().canonicalize())
        .collect::<Result<Vec<_>, _>>()?;

    let sep = if cfg!(windows) { ";" } else { ":" };
    Ok(canonicalized
        .iter()
        .map(|p| {
            let s = p.to_string_lossy();
            // Strip Windows \\?\ extended-length prefix that R can't handle
            s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
        })
        .collect::<Vec<_>>()
        .join(sep))
}

/// By default, doing ctrl+c on rv will kill it as well as all its child process.
/// To allow graceful shutdown, we create a process group in Unix and the equivalent on Windows
/// so we can control _how_ they get killed, and allow for a soft cancellation (eg we let
/// ongoing tasks finish but stop enqueuing/processing new ones.
fn spawn_isolated_r_command(r_cmd: &RInstall) -> Command {
    let mut command = Command::new(&r_cmd.bin_path);

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

/// Spawns a prepared R command with output capture, PID tracking, and cancellation support.
/// Returns captured output on success. On failure, calls `on_failure` with the output to
/// produce the appropriate error kind.
fn run_r_command(
    mut command: Command,
    cancellation: Arc<Cancellation>,
    on_failure: impl FnOnce(String) -> RCmdErrorKind,
) -> Result<String, RCmdError> {
    let (recv, send) = std::io::pipe().map_err(|e| RCmdError {
        source: RCmdErrorKind::Command(e),
    })?;
    command
        .stdout(send.try_clone().map_err(|e| RCmdError {
            source: RCmdErrorKind::Command(e),
        })?)
        .stderr(send);

    let mut handle = command.spawn().map_err(|e| RCmdError {
        source: RCmdErrorKind::Command(e),
    })?;

    let pid = handle.id();

    {
        let mut process_ids = ACTIVE_R_PROCESS_IDS.lock().unwrap();
        process_ids.insert(pid);
    }

    // could deadlock otherwise
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

    // Poll for completion or cancellation
    loop {
        match handle.try_wait() {
            Ok(Some(status)) => {
                {
                    let mut process_ids = ACTIVE_R_PROCESS_IDS.lock().unwrap();
                    process_ids.remove(&pid);
                }
                let output = output_handle.join().unwrap();

                if !status.success() {
                    return Err(RCmdError {
                        source: on_failure(output),
                    });
                }

                return Ok(output);
            }
            Ok(None) => {
                if cancellation.is_soft_cancellation() {
                    // On soft cancellation, let R finish naturally
                    // On hard cancellation, rv will kill
                    let status = handle.wait().unwrap();
                    let output = output_handle.join().unwrap();

                    {
                        let mut process_ids = ACTIVE_R_PROCESS_IDS.lock().unwrap();
                        process_ids.remove(&pid);
                    }

                    if !status.success() {
                        return Err(RCmdError {
                            source: on_failure(output),
                        });
                    }

                    return Ok(output);
                }

                // Sleep briefly to avoid busy waiting
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(RCmdError {
                    source: RCmdErrorKind::Command(e),
                });
            }
        }
    }
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

impl RCmd for RInstall {
    fn install(
        &self,
        source_folder: impl AsRef<Path>,
        sub_folder: Option<impl AsRef<Path>>,
        libraries: &[impl AsRef<Path>],
        destination: impl AsRef<Path>,
        cancellation: Arc<Cancellation>,
        env_vars: &HashMap<&str, &str>,
        configure_args: &[String],
        strip: bool,
    ) -> Result<String, RCmdError> {
        let destination = destination.as_ref();
        // We create a temp build dir so we only remove an existing destination if we have something we can replace it with
        let build_dir = tempfile::tempdir().map_err(|e| RCmdError {
            source: RCmdErrorKind::TempDir(e),
        })?;

        // We move the source to a temp dir since compilation might create a lot of artifacts that
        // we don't want to keep around in the cache once we're done
        // We symlink if possible except on Windows
        let src_backup_dir_temp = tempfile::tempdir().map_err(|e| RCmdError {
            source: RCmdErrorKind::TempDir(e),
        })?;

        let mut src_backup_dir = src_backup_dir_temp.path().to_owned();

        LinkMode::link_files(
            Some(LinkMode::Copy),
            "tmp_build",
            &source_folder,
            &src_backup_dir,
        )
        .map_err(|e| RCmdError {
            source: RCmdErrorKind::LinkError(e),
        })?;

        // Some R package structures, especially those that make use of
        // bootstrap.R like tree-sitter-r require the parent directories
        // to exist during build. We need to copy the whole repo
        // and install from the subdirectory directly
        if let Some(sub_dir) = sub_folder {
            src_backup_dir.push(sub_dir);
        }

        let library_paths =
            r_library_paths(libraries).map_err(|e| RCmdError::from_fs_io(e, destination))?;

        let mut command = spawn_isolated_r_command(self);
        command
            .arg("CMD")
            .arg("INSTALL")
            // This is where it will be installed
            .arg(format!(
                "--library={}",
                build_dir.as_ref().to_string_lossy()
            ))
            .arg("--use-vanilla");

        if strip {
            command.arg("--strip").arg("--strip-lib");
        }

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
            .arg(&src_backup_dir)
            // Override where R should look for deps
            .env("R_LIBS_SITE", &library_paths)
            .env("R_LIBS_USER", &library_paths);

        if strip {
            command.env("_R_SHLIB_STRIP_", "true");
        }

        command.envs(env_vars);
        log::debug!(
            "Compiling {} with env vars: {}",
            source_folder.as_ref().display(),
            command
                .get_envs()
                .map(|(k, v)| format!(
                    "{}={}",
                    k.to_string_lossy(),
                    v.unwrap_or_default().to_string_lossy()
                ))
                .collect::<Vec<_>>()
                .join(" ")
        );

        let output = run_r_command(
            command,
            cancellation,
            RCmdErrorKind::InstallationFailed,
        )
        .map_err(|e| {
            // Clean up destination on failure
            if destination.is_dir() {
                if let Err(rm_err) = fs::remove_dir_all(destination) {
                    log::error!(
                        "Failed to remove directory `{}` after R CMD INSTALL failed: {rm_err}. Delete this folder manually",
                        destination.display()
                    );
                }
            }
            e
        })?;

        // Copy the build tmp dir to the actual destination
        // we don't move the folder since the tmp dir might be in another drive/format
        // than the cache dir
        fs::create_dir_all(destination).map_err(|e| RCmdError::from_fs_io(e, destination))?;
        copy_folder(build_dir.as_ref(), destination)
            .map_err(|e| RCmdError::from_fs_io(e, destination))?;

        Ok(output)
    }

    fn build(
        &self,
        source_dir: impl AsRef<Path>,
        output_dir: impl AsRef<Path>,
        libraries: &[impl AsRef<Path>],
        cancellation: Arc<Cancellation>,
        env_vars: &HashMap<&str, &str>,
    ) -> Result<PathBuf, RCmdError> {
        let output_dir = output_dir.as_ref();
        let source_dir = source_dir.as_ref();

        let library_paths =
            r_library_paths(libraries).map_err(|e| RCmdError::from_fs_io(e, source_dir))?;

        let mut command = spawn_isolated_r_command(self);
        command
            .arg("CMD")
            .arg("build")
            .arg("--no-build-vignettes")
            .arg("--no-manual")
            .arg(source_dir)
            .current_dir(output_dir)
            .env("R_LIBS_SITE", &library_paths)
            .env("R_LIBS_USER", &library_paths)
            .envs(env_vars);

        log::debug!("Running R CMD build on {}", source_dir.display());

        let output = run_r_command(command, cancellation, RCmdErrorKind::BuildFailed)?;

        // Find the produced tarball in output_dir
        let tarball = fs::read_dir(output_dir)
            .map_err(|e| RCmdError::from_fs_io(e, output_dir))?
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.path().extension().is_some_and(|ext| ext == "gz"))
            .map(|entry| entry.path())
            .ok_or_else(|| RCmdError {
                source: RCmdErrorKind::BuildFailed(format!(
                    "R CMD build succeeded but no tarball found in {}.\nOutput: {}",
                    output_dir.display(),
                    output
                )),
            })?;

        Ok(tarball)
    }

    fn get_r_library(&self) -> Result<PathBuf, LibraryError> {
        let r_home = get_r_home(&self.bin_path).map_err(|e| LibraryError {
            source: LibraryErrorKind::Io(e),
        })?;

        let lib_path = r_home.join("library");

        if lib_path.is_dir() {
            Ok(lib_path)
        } else {
            Err(LibraryError {
                source: LibraryErrorKind::NotFound,
            })
        }
    }

    fn version(&self) -> Result<Option<Version>, VersionError> {
        let output = Command::new(&self.bin_path)
            .arg("--version")
            .output()
            .map_err(|e| VersionError {
                source: VersionErrorKind::Io(e),
            })?;

        let stdout = r_output_str(&output).map_err(|e| VersionError {
            source: VersionErrorKind::Utf8(e),
        })?;

        if is_r_devel(stdout) {
            return Ok(None);
        }
        // If we don't find either a devel or a version number, assume we didn't find R
        find_r_version(stdout).map(Some).ok_or(VersionError {
            source: VersionErrorKind::NotFound,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
#[non_exhaustive]
pub struct RCmdError {
    pub source: RCmdErrorKind,
}

impl RCmdError {
    pub fn from_fs_io(error: std::io::Error, path: &Path) -> Self {
        Self {
            source: RCmdErrorKind::File {
                error,
                path: path.to_path_buf(),
            },
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RCmdErrorKind {
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
    #[error("{0}")]
    InstallationFailed(String),
    #[error("R CMD build failed:\n{0}")]
    BuildFailed(String),
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
    #[error("Library for current R not found")]
    NotFound,
}

/// On Windows, R may write to stdout or stderr depending on how it's invoked
/// (R.bat vs R.exe), so check both. On other platforms, just use stdout.
fn r_output_str(output: &std::process::Output) -> Result<&str, std::str::Utf8Error> {
    if cfg!(windows) {
        let stdout = std::str::from_utf8(&output.stdout)?;
        if stdout.trim().is_empty() {
            std::str::from_utf8(&output.stderr)
        } else {
            Ok(stdout)
        }
    } else {
        std::str::from_utf8(&output.stdout)
    }
}

pub(crate) fn get_r_home(r_bin_path: &Path) -> Result<PathBuf, std::io::Error> {
    let output = Command::new(r_bin_path)
        .arg("RHOME")
        .env_remove("R_HOME")
        .output()?;

    let r_home = r_output_str(&output)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
        .trim();

    Ok(PathBuf::from(r_home))
}

#[cfg(test)]
mod tests {
    use super::find_r_version;
    use crate::Version;

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
    fn can_handle_devel() {
        let r_response = r#"/
R Under development (unstable) (2025-10-22 r88969) -- "Unsuffered Consequences"
Copyright (C) 2025 The R Foundation for Statistical Computing
Platform: aarch64-apple-darwin20

R is free software and comes with ABSOLUTELY NO WARRANTY.
You are welcome to redistribute it under the terms of the
GNU General Public License versions 2 or 3.
For more information about these matters see
https://www.gnu.org/licenses/."#;
        assert!(find_r_version(r_response).is_none());
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
