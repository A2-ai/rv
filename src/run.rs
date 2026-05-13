use std::path::{Path, PathBuf};

/// R environment variables to remove before spawning Rscript.
/// R_LIBS is cleared so only the project library is used.
const R_ENV_VARS_TO_REMOVE: &[&str] = &["R_LIBS", "R_INCLUDE_DIR", "R_SHARE_DIR", "R_DOC_DIR"];

/// Run `Rscript` with the given arguments and the project library paths configured.
pub fn run(r_bin_path: &Path, library_path: &Path, args: &[String]) -> Result<i32, RunError> {
    let r_home = crate::r_cmd::get_r_home(r_bin_path).map_err(|source| RunError::RHome {
        path: r_bin_path.to_path_buf(),
        source,
    })?;
    let rscript = resolve_rscript_path(&r_home, r_bin_path);

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

fn resolve_rscript_path(r_home: &Path, r_bin_path: &Path) -> PathBuf {
    let rscript = r_home.join("bin").join("Rscript");

    #[cfg(not(windows))]
    let _ = r_bin_path;

    #[cfg(windows)]
    {
        let mut rscript = rscript;
        if let Some(ext) = r_bin_path.extension() {
            rscript.set_extension(ext);
            return rscript;
        }

        rscript.set_extension("exe");
        return rscript;
    }

    rscript
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

#[cfg(test)]
mod tests {
    use super::resolve_rscript_path;
    use std::path::PathBuf;

    #[test]
    fn resolve_rscript_from_r_home_when_r_has_no_parent() {
        let r_home = PathBuf::from("/opt/R/4.5.0/lib/R");
        let r_bin_path = PathBuf::from("R");

        #[cfg(not(windows))]
        assert_eq!(
            resolve_rscript_path(&r_home, &r_bin_path),
            r_home.join("bin").join("Rscript")
        );

        #[cfg(windows)]
        assert_eq!(
            resolve_rscript_path(&r_home, &r_bin_path),
            r_home.join("bin").join("Rscript.exe")
        );
    }
}
