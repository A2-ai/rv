use std::path::{Path, PathBuf};

use crate::consts::RUN_ACTIVE_ENV_VAR_NAME;
use crate::r_cmd::StartupFiles;

/// R environment variables to remove before spawning Rscript.
/// R_LIBS is cleared so only the project library is used.
const R_ENV_VARS_TO_REMOVE: &[&str] = &["R_LIBS", "R_INCLUDE_DIR", "R_SHARE_DIR", "R_DOC_DIR"];

/// Run `Rscript` with the given arguments and the project library paths configured.
pub fn run(
    r_bin_path: &Path,
    library_path: &Path,
    sandbox: Option<&Path>,
    isolated: bool,
    args: &[String],
) -> Result<i32, RunError> {
    let r_home = crate::r_cmd::get_r_home(r_bin_path).map_err(|source| RunError::RHome {
        path: r_bin_path.to_path_buf(),
        source,
    })?;
    let rscript = crate::r_cmd::resolve_rscript_path(&r_home);

    // We need libraries path to be absolute so it works in all contexts of a script
    let library_path =
        std::path::absolute(library_path).map_err(|source| RunError::LibraryPath {
            path: library_path.to_path_buf(),
            source,
        })?;

    let mut cmd = std::process::Command::new(&rscript);

    // Kept around until the script is over since we need to keep the temp files in it
    let _files = if sandbox.is_some() || isolated {
        let startup = StartupFiles::write(sandbox).map_err(RunError::Startup)?;
        startup.apply_profile(&mut cmd);
        if isolated {
            startup.apply_isolation(&mut cmd);
        }
        Some(startup)
    } else {
        None
    };

    cmd.args(args)
        .env("R_HOME", &r_home)
        .env("R_LIBS_USER", &library_path)
        .env("R_LIBS_SITE", &library_path)
        // Tells a project activate script that the library and sandbox are already set up
        .env(RUN_ACTIVE_ENV_VAR_NAME, "1")
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    for var in R_ENV_VARS_TO_REMOVE {
        cmd.env_remove(var);
    }

    let status = cmd.status().map_err(|source| RunError::Spawn {
        path: rscript,
        source,
    })?;

    Ok(status.code().unwrap_or(1))
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("Failed to run Rscript at {path}: {source}")]
    Spawn {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to determine R_HOME from {path}: {source}")]
    RHome {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to resolve the library {path} to an absolute path: {source}")]
    LibraryPath {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to create the R startup files: {0}")]
    Startup(std::io::Error),
}
