use std::io::Error;
use std::path::Path;
use std::process::Command;
use std::str;
use std::sync::LazyLock;

use crate::version::Version;
use regex::Regex;

static R_VERSION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+)\.(\d+)\.(\d+)").unwrap());

fn find_r_version(output: &str) -> Version {
    R_VERSION_RE
        .captures(output)
        .unwrap()
        .get(0)
        .unwrap()
        .as_str()
        .parse::<Version>()
        .unwrap()
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

    // TODO: this should return Result<Version> instead
    fn version(&self) -> Version;
}

pub struct RCommandLine;

impl RCmd for RCommandLine {
    fn install(
        &self,
        _file_path: &Path,
        _library: &Path,
        _args: Vec<&str>,
        _env_var: Vec<(&str, &str)>,
    ) -> Result<(), Error> {
        todo!()
    }

    fn check(
        &self,
        _file_path: &Path,
        _result_path: &Path,
        _args: Vec<&str>,
        _env_var: Vec<(&str, &str)>,
    ) -> Result<(), Error> {
        todo!()
    }

    fn build(
        &self,
        _file_path: &Path,
        _library: &Path,
        _output_path: &Path,
        _args: Vec<&str>,
        _env_var: Vec<(&str, &str)>,
    ) -> Result<(), Error> {
        todo!()
    }

    fn version(&self) -> Version {
        let output = Command::new("R").arg("--version").output().unwrap();
        let stdout = str::from_utf8(&output.stdout).unwrap();
        find_r_version(stdout)
    }
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
            find_r_version(r_response),
            "4.4.1".parse::<Version>().unwrap()
        )
    }

    #[test]
    #[should_panic]
    fn r_not_found() {
        let r_response = r#"/
Command 'R' is available in '/usr/local/bin/R'
The command could not be located because '/usr/local/bin' is not included in the PATH environment variable.
R: command not found"#;
        find_r_version(r_response);
    }
}
