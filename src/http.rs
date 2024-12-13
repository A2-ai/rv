use std::{env, fs::{self, File}, io::Write, process::{self, Command}};
use reqwest::{blocking::Client, header::{HeaderMap, HeaderValue, USER_AGENT}};

pub struct Package {
    name: String,
    version: String,
    url: String,
}

impl Package {
    pub fn test_package() -> Self {
        Package{
            name: "R6".to_string(),
            version: "2.5.1".to_string(),
            url: "https://packagemanager.posit.co/cran/__linux__/jammy/2024-12-04".to_string(),
        }
    }

    pub fn download(&self, r_version: &str) -> String {
        //let user_agent = r_command(r#"cat(getOption("HTTPUserAgent")"#);
        let home_path = env::var("HOME").or_else(|_| env::var("USERPROFILE")).expect("TODO: handle cannot find home directory error");
        let file_path = format!("{}/.cache/rv/R/{}-library/{}", 
            home_path, 
            r_command(r#"cat(R.version$platform)"#),
            r_version
        );
        fs::create_dir_all(&file_path).expect("TODO: handle cache dir can't be created");

        self.download_file(file_path)
    }

    fn download_file(&self, file_path: String) -> String {
        let user_agent: String = r_command(r#"cat(getOption('HTTPUserAgent'))"#);
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::try_from(user_agent).unwrap());
            
        let client = Client::builder().default_headers(headers).build().expect("TODO: handle header build error");
        let response = client.get(self.get_file_url()).send().expect("TODO: handle get file error");

        if !response.status().is_success() {
            eprintln!("TODO: handle response failed: {}", response.status());
            process::exit(1);
        }
        let content = response.bytes()
            .expect("TODO: handle convert to bytes error");

        let output_file = format!("{}/{}", file_path.clone(), self.package_version().clone());
        let mut file = File::create(&output_file)
            .expect("TODO: handle creating output file");
        file.write_all(&content)
            .expect("TODO: handle failed to write file error");
        
        output_file
    }

    #[cfg(target_os = "linux")]
    fn get_file_url(&self) -> String {
        format!("{}/src/contrib/{}", self.url, self.package_version())
    }

    #[cfg(not(target_os = "linux"))]
    fn get_file_url(&self) -> String {
        "TODO: get different urls for each OS".to_string();
    }

    fn package_version(&self) -> String {
        format!("{}_{}.tar.gz", self.name, self.version)
    }
}

fn r_command(command: &str) -> String {
    println!("{command}");
    let output = Command::new("Rscript")
        .arg("-e")
        .arg(command)
        .output()
        .expect("TODO: 1. handle command not run error");

    if !output.status.success() { eprintln!("TODO: 2. handle command failed error") };
    String::from_utf8_lossy(&output.stdout).to_string()
}

mod tests {
    use super::*;

    #[test]
    fn can_download_tarball() {
        let download_path = Package::test_package().download(&r_command(r#"cat(paste(R.version$major, R.version$minor, sep = "."))"#));
        if let Ok(exists) = fs::exists(download_path) {
            assert!(exists)
        } else {
            panic!("Error")
        }
    }
}
