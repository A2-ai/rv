use std::path::{Path, PathBuf};

use crate::r_cmd::r_library_paths;

/// R environment variables to remove before spawning Rscript.
/// R_LIBS is cleared so only the project library is used.
const R_ENV_VARS_TO_REMOVE: &[&str] = &["R_LIBS", "R_INCLUDE_DIR", "R_SHARE_DIR", "R_DOC_DIR"];
const PROJECT_LIBRARY_ENV_VAR: &str = "RV_PROJECT_LIBRARY";
const SANDBOX_LIBRARY_ENV_VAR: &str = "RV_SANDBOX_LIBRARY";

const SANDBOX_PROFILE: &str = r#"local({
	project <- Sys.getenv("RV_PROJECT_LIBRARY", unset = "")
	sandbox <- Sys.getenv("RV_SANDBOX_LIBRARY", unset = "")
	if (!nzchar(project) || !dir.exists(project)) {
		stop("rv project library is unavailable", call. = FALSE)
	}
	if (!nzchar(sandbox) || !dir.exists(sandbox)) {
		stop("rv sandbox is enabled but its library is unavailable", call. = FALSE)
	}
	project <- normalizePath(project, winslash = "/", mustWork = TRUE)
	sandbox <- normalizePath(sandbox, winslash = "/", mustWork = TRUE)

	env <- baseenv()
	if (bindingIsLocked(".Library", env)) unlockBinding(".Library", env)
	assign(".Library", sandbox, envir = env)
	lockBinding(".Library", env)
	if (bindingIsLocked(".Library.site", env)) unlockBinding(".Library.site", env)
	assign(".Library.site", character(), envir = env)
	lockBinding(".Library.site", env)

	.libPaths(c(project, sandbox), include.site = FALSE)
	paths <- paste(project, sandbox, sep = .Platform$path.sep)
	Sys.setenv(R_LIBS_USER = paths, R_LIBS_SITE = paths)
})
"#;

/// Run `Rscript` with the given arguments and the project library paths configured.
pub fn run(r_bin_path: &Path, library_path: &Path, args: &[String]) -> Result<i32, RunError> {
    run_with_sandbox(r_bin_path, library_path, None, false, args)
}

/// Run `Rscript` with the project library and optional system-library sandbox.
///
/// The sandbox is established by a controlled site profile before R loads the
/// normal project or user `.Rprofile`. In isolated mode, user and site startup
/// files are suppressed.
pub fn run_with_sandbox(
    r_bin_path: &Path,
    library_path: &Path,
    sandbox_path: Option<&Path>,
    isolated: bool,
    args: &[String],
) -> Result<i32, RunError> {
    let r_home = crate::r_cmd::get_r_home(r_bin_path).map_err(|source| RunError::RHome {
        path: r_bin_path.to_path_buf(),
        source,
    })?;
    let rscript = crate::r_cmd::resolve_rscript_path(&r_home);

    let mut libraries = vec![library_path];
    if let Some(sandbox_path) = sandbox_path {
        libraries.push(sandbox_path);
    }
    let library_paths = r_library_paths(&libraries).map_err(|source| RunError::LibraryPaths {
        path: library_path.to_path_buf(),
        source,
    })?;

    let startup = if sandbox_path.is_some() {
        let directory = tempfile::tempdir().map_err(|source| RunError::Startup { source })?;
        std::fs::write(directory.path().join("site.Rprofile"), SANDBOX_PROFILE)
            .map_err(|source| RunError::Startup { source })?;
        std::fs::write(directory.path().join("empty"), "")
            .map_err(|source| RunError::Startup { source })?;
        Some(directory)
    } else {
        None
    };

    let mut cmd = std::process::Command::new(&rscript);
    cmd.args(args)
        .env("R_HOME", &r_home)
        .env("R_LIBS_USER", &library_paths)
        .env("R_LIBS_SITE", &library_paths)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());

    for var in R_ENV_VARS_TO_REMOVE {
        cmd.env_remove(var);
    }

    if let (Some(sandbox_path), Some(startup)) = (sandbox_path, &startup) {
        cmd.env(PROJECT_LIBRARY_ENV_VAR, library_path)
            .env(SANDBOX_LIBRARY_ENV_VAR, sandbox_path)
            .env("R_PROFILE", startup.path().join("site.Rprofile"));
        if isolated {
            let empty = startup.path().join("empty");
            cmd.env("R_ENVIRON", &empty)
                .env("R_ENVIRON_USER", &empty)
                .env("R_PROFILE_USER", &empty);
        }
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
    #[error("Failed to configure R library paths from {path}: {source}")]
    LibraryPaths {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Failed to create controlled R startup files: {source}")]
    Startup { source: std::io::Error },
}
