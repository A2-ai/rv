use std::path::{Path, PathBuf};

/// Run `Rscript` with the given arguments and the project library paths configured.
pub fn run(r_bin_path: &Path, library_path: &Path, args: &[String]) -> Result<i32, RunError> {
    let rscript = r_bin_path
        .parent()
        .map(|p| p.join("Rscript"))
        .unwrap_or_else(|| PathBuf::from("Rscript"));

    let library_path = library_path.to_string_lossy();
    let status = std::process::Command::new(&rscript)
        .args(args)
        .env("R_LIBS", "")
        .env("R_LIBS_USER", &*library_path)
        .env("R_LIBS_SITE", &*library_path)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|source| RunError::Spawn {
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
}
