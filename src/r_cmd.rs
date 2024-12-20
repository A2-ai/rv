use std::path::Path;
use regex::Regex;
use once_cell::sync::Lazy;

static R_VERSION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(\d+)\.(\d+)\.(\d+)"
    )
    .unwrap()
});
pub trait RCmd {
    fn install(
        &self, 
        file_path: &Path,
        library: &Path, 
        args: Vec<&str>, 
        env_var: Vec<(&str, &str)>
    ) -> Result<(), std::io::Error>;

    fn check(
        &self, 
        file_path: &Path,
        result_path: &Path, 
        args: Vec<&str>, 
        env_var: Vec<(&str, &str)>
    ) -> Result<(), std::io::Error>;

    fn build(
        &self, 
        file_path: &Path,
        library: &Path,
        output_path: &Path, 
        args: Vec<&str>, 
        env_var: Vec<(&str, &str)>
    ) -> Result<(), std::io::Error>;

    fn version(&self) -> String;
}

#[allow(unused_imports, unused_variables)]
mod tests {
    use crate::version::Version;
    use super::*;

    fn find_r_version(output: String) -> Version {
        R_VERSION_RE
            .captures(&output)
            .unwrap()
            .get(0)
            .unwrap()
            .as_str()
            .parse::<Version>()
            .unwrap()
    }

    struct TestRCmd (String);
    impl RCmd for TestRCmd {
        fn install(&self, file_path: &Path,library: &Path, args: Vec<&str>, env_var: Vec<(&str, &str)>) -> Result<(), std::io::Error> { Ok(()) }

        fn build(&self, file_path: &Path,library: &Path,output_path: &Path, args: Vec<&str>, env_var: Vec<(&str, &str)>) -> Result<(), std::io::Error> { Ok(()) }

        fn check(&self, file_path: &Path,result_path: &Path, args: Vec<&str>, env_var: Vec<(&str, &str)>) -> Result<(), std::io::Error> { Ok(()) }

        fn version(&self) -> String {
            self.0.clone()
        }
    }
    #[test]
    fn can_read_r_version() {
        let r_response = format!(r#"/
R version 4.4.1 (2024-06-14) -- "Race for Your Life"
Copyright (C) 2024 The R Foundation for Statistical Computing
Platform: x86_64-pc-linux-gnu

R is free software and comes with ABSOLUTELY NO WARRANTY.
You are welcome to redistribute it under the terms of the
GNU General Public License versions 2 or 3.
For more information about these matters see
https://www.gnu.org/licenses/."#);
        let ver_str = TestRCmd(r_response).version();
        assert_eq!(find_r_version(ver_str), "4.4.1".parse::<Version>().unwrap())
    }

    #[test]
    #[should_panic]
    fn r_not_found() {
        let r_response = format!(r#"/
Command 'R' is available in '/usr/local/bin/R'
The command could not be located because '/usr/local/bin' is not included in the PATH environment variable.
R: command not found"#);
        let ver_str = TestRCmd(r_response).version();
        find_r_version(ver_str);
    }

}