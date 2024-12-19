use std::path::Path;
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
}

#[allow(unused_imports, unused_variables)]
mod tests {
    use super::*;

    struct TestRCmd ();
    impl RCmd for TestRCmd {
        fn install(
                &self, 
                file_path: &Path,
                library: &Path, 
                args: Vec<&str>, 
                env_var: Vec<(&str, &str)>
            ) -> Result<(), std::io::Error> {
            Ok(())
        }

        fn build(
                &self, 
                file_path: &Path,
                library: &Path,
                output_path: &Path, 
                args: Vec<&str>, 
                env_var: Vec<(&str, &str)>
            ) -> Result<(), std::io::Error> {
            Ok(())
        }

        fn check(
                &self, 
                file_path: &Path,
                result_path: &Path, 
                args: Vec<&str>, 
                env_var: Vec<(&str, &str)>
            ) -> Result<(), std::io::Error> {
            Ok(())
        }
    }
    #[test]
    fn check_install() {
        TestRCmd().install(Path::new(&""), Path::new(&""), Vec::new(), Vec::new()).unwrap();
    }

    #[test]
    fn check_build() {
        TestRCmd().build(Path::new(""), Path::new(""), Path::new(""), Vec::new(), Vec::new()).unwrap();
    }

    #[test]
    fn check_check() {
        TestRCmd().check(Path::new(""), Path::new(""), Vec::new(), Vec::new()).unwrap();
    }

}