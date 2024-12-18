use std::{process::{Command, Output}, path::Path, fs};
use crate::r_cmd::RCmd;
struct Package {
    tarball_path: String
}

impl RCmd for Package {
    fn install(&self, library: &Path, args: Option<Vec<&str>>, env_var: Option<Vec<(&str, &str)>>) -> Result<(), std::io::Error> {
        let mut command = Command::new("R");
        command
            .arg("CMD")
            .arg("INSTALL")
            .arg(&self.tarball_path)
            .arg("-l")
            .arg(library);

        if let Some(args) = args {
            command.args(args);
        } else {
            command.arg("--use-vanilla");
        }

        if let Some(env_vars) = env_var {
            command
                .envs(env_vars.iter().map(|(key, val)| (key, val)));
            //TODO: canonicalize val if its a path
        }

        command.status()?;
        Ok(())
    }

    fn check(&self, result_path: &Path, args: Option<Vec<&str>>, env_var: Option<Vec<(&str, &str)>>) -> Result<(), std::io::Error> {
        let result_path = fs::canonicalize(result_path).unwrap();
        let tarball_path = fs::canonicalize(&self.tarball_path).unwrap();
        let mut command = Command::new("R");
        command
            .arg("CMD")
            .arg("check")
            .arg(tarball_path)
            .arg("-o")
            .arg(result_path);

        if let Some(args) = args {
            command.args(args);
        } else {
            command.arg("--use-vanilla");
        }

        if let Some(env_vars) = env_var {
            command
                .envs(env_vars.iter().map(|(key, val)| (key, val)));
            //TODO: canonicalize val if its a path
        }

        command.status()?;
        Ok(())
    }

    fn build(&self, library: &Path, output_path: &Path, args: Option<Vec<&str>>, env_var: Option<Vec<(&str, &str)>>) -> Result<(), std::io::Error> {
        let mut command = Command::new("R");
        command
            .arg("CMD")
            .arg("INSTALL")
            .arg("--build")
            .arg(fs::canonicalize(&self.tarball_path).unwrap())
            .arg("-l")
            .arg(fs::canonicalize(library).unwrap());

        if let Some(args) = args {
            command.args(args);
        }  else {
            command.arg("--use-vanilla");
        }

        if let Some(env_vars) = env_var {
            command
                .envs(env_vars.iter().map(|(key, val)| (key, val)));
            //TODO: canonicalize val if its a path
        }
        command.current_dir(output_path);

        println!("{:#?}", command);

        command.status()?;
        Ok(())
    }
}

mod tests {
    use std::{path::Path, vec};
    use super::{Package, RCmd};

    fn package() -> Package {
        Package{ tarball_path: String::from("./src/tests/RCMD/R6_2.5.1.tar.gz") }
    }

    #[test]
    fn can_install_no_config() {
        package()
            .install(&Path::new("./src/tests/RCMD/"), None, None);
    }

    #[test]
    fn can_install_with_config() {
        package()
            .install(
                &Path::new("./src/tests/RCMD/"), 
                Some(vec!["--debug"]), 
                Some(vec![("R_LIBS_USER", "./src/tests/RCMD/user")])
            ).unwrap();
    }

    #[test]
    fn can_build() {
        package()
            .build(&Path::new("./src/tests/RCMD"), 
            &Path::new("./src/tests/RCMD"),
            None, 
            None
        ).unwrap();
    }

    #[test]
    fn can_check() {
        package()
            .check(&Path::new("./src/tests/RCMD"), None, None)
            .unwrap();
    }
}