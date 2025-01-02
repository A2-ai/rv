use std::path::Path;
use std::process::Command;
use std::str::FromStr;
use std::sync::LazyLock;

use regex::Regex;

use crate::version::Version;

static R_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap());

fn find_r_version(output: &str) -> Option<Version> {
    R_VERSION_RE
        .captures(output)
        .and_then(|c| c.get(0))
        .and_then(|m| Version::from_str(m.as_str()).ok())
}

pub trait RCmd {
    fn install(
        &self,
        file_path: &Path,
        library: &Path,
        args: Vec<&str>,
        env_var: Vec<(&str, &str)>,
    ) -> Result<(), std::io::Error>;

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
        _file_path: &Path,
        _library: &Path,
        _args: Vec<&str>,
        _env_var: Vec<(&str, &str)>,
    ) -> Result<(), std::io::Error> {
        todo!()
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
