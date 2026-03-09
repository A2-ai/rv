use std::path::{Path, PathBuf};

/// R environment variables to remove before spawning Rscript.
/// R_LIBS is cleared so only the project library is used.
const R_ENV_VARS_TO_REMOVE: &[&str] = &["R_LIBS", "R_INCLUDE_DIR", "R_SHARE_DIR", "R_DOC_DIR"];

/// Run `Rscript` with the given arguments and the project library paths configured.
pub fn run(r_bin_path: &Path, library_path: &Path, args: &[String]) -> Result<i32, RunError> {
    let rscript = r_bin_path
        .parent()
        .map(|p| p.join("Rscript"))
        .unwrap_or_else(|| PathBuf::from("Rscript"));

    let r_home = crate::r_cmd::get_r_home(r_bin_path).map_err(|source| RunError::RHome {
        path: r_bin_path.to_path_buf(),
        source,
    })?;

    let mut cmd = std::process::Command::new(&rscript);
    cmd.args(args)
        .env("R_HOME", &r_home)
        .env("R_LIBS_USER", library_path)
        .env("R_LIBS_SITE", library_path)
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
}
