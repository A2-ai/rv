use std::{env::{set_current_dir, set_var}, fs, process::Command};

#[allow(non_camel_case_types)]
trait R_CMD {
    fn r_libs_user(&self) -> &str;
    fn r_libs_site(&self) -> &str;
    fn pkg_path(&self) -> &str;
    // Using 1 output_dir arg for check and build, despite the outputs being very different. 
    // `check` is not part of the standard work stream, so should be fine?
    fn output_dir(&self) -> Option<&str>;
    

    fn install(&self) {
        set_var("R_LIBS_USER", self.r_libs_user());
        set_var("R_LIBS_SITE", self.r_libs_site());
        let output = Command::new("R")
            .arg("CMD")
            .arg("INSTALL")
            .arg(self.pkg_path())
            .output()
            .expect("TODO: R CMD INSTALL failed");

        if !output.status.success() { panic!("TODO: handle not successful R CMD install") }
    }

    fn check(&self) {
        set_var("R_LIBS_USER", self.r_libs_user());
        set_var("R_LIBS_SITE", self.r_libs_site());
        let output = Command::new("R")
            .arg("CMD")
            .arg("check")
            .arg(self.pkg_path())
            .arg("-o")
            .arg(self.output_dir().unwrap())
            .output()
            .expect("TODO: R CMD check failed");
        
        if !output.status.success() { eprintln!("R CMD check did not pass") }
    }

    fn build(&self) {
        set_var("R_LIBS_USER", self.r_libs_user());
        set_var("R_LIBS_SITE", self.r_libs_site());
        let pkg_path = fs::canonicalize(self.pkg_path()).unwrap(); // setting to absolute path as setting current dir changes relative paths
        set_current_dir(self.output_dir().unwrap()).unwrap(); // setting for built binary to be saved to specified dir
        let output = Command::new("R")
            .arg("CMD")
            .arg("INSTALL")
            .arg("--build")
            .arg(pkg_path)
            .output()
            .expect("TODO: R CMD INSTALL --build failed");

        if !output.status.success() { panic!("TODO: handle not successful R CMD build") }
    }
}

mod test {
    // tests were written for me to test as I worked, not as actual unit tests. Will clean before merging
    use super::*;

    struct TestStruct {
        r_libs_user: String,
        r_libs_site: String,
        path: String,
        output_dir: Option<String>,
    }
    
    impl R_CMD for TestStruct { 
        fn r_libs_site(&self) -> &str { &self.r_libs_site }
        fn r_libs_user(&self) -> &str { &self.r_libs_user }
        fn pkg_path(&self) -> &str { &self.path }
        fn output_dir(&self) -> Option<&str> { self.output_dir.as_deref() }
    }

    #[test]
    fn can_install() {
        TestStruct{
            r_libs_user: "/cluster-data/user-homes/wes/R/persieve".to_string(),
            r_libs_site: "/opt/R/4.4.1/lib/R/library".to_string(),
            path: "./src/tests/RCMD/R6_2.5.1.tar.gz".to_string(),
            output_dir: None,
        }.install();
    }

    #[test]
    fn can_check() {
        TestStruct{
            r_libs_user: "/cluster-data/user-homes/wes/R/persieve".to_string(),
            r_libs_site: "/opt/R/4.4.1/lib/R/library".to_string(),
            path: "./src/tests/RCMD/R6_2.5.1.tar.gz".to_string(),
            output_dir: Some("./src/tests/RCMD/".to_string()),
        }.check();
    }

    #[test]
    fn can_build() {
        TestStruct{
            r_libs_user: "/cluster-data/user-homes/wes/R/persieve".to_string(),
            r_libs_site: "/opt/R/4.4.1/lib/R/library".to_string(),
            path: "./src/tests/RCMD/R6_2.5.1.tar.gz".to_string(),
            output_dir: Some("./src/tests/RCMD".to_string()),
        }.build();
    }
}