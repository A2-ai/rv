use std::{fs::{self, File}, io::Write, process};
use reqwest::{blocking::{Response, Client}, header::{HeaderMap, HeaderValue, USER_AGENT}};

mod binaries;
mod user_agent;

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
        let xdg_dirs = xdg::BaseDirectories::with_prefix("rv").expect("TODO: handle error");
        let cache_base_dir = xdg_dirs.get_cache_home();
        let os = os_info::get();
        let platform = format!("{}-{}-{}", 
            os.architecture().unwrap(), 
            os.os_type(),
            os.version());
        let file_path = format!("{}/.cache/rv/R/{}-library/{}", 
            cache_base_dir.display(), 
            platform,
            r_version
        );
        fs::create_dir_all(&file_path).expect("TODO: handle cache dir can't be created");
        self.download_file(file_path)
    }

    fn download_file(&self, file_path: String) -> String {
        let user_agent = user_agent::new();
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::try_from(user_agent).unwrap());
            
        let client = Client::builder().default_headers(headers).build().expect("TODO: handle header build error");
        
        let response = self.get_url_response(client);

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
    fn get_url_response(&self, client: Client) -> Response {
        //moved url response to separate function to handle linux binaries

        let url = format!("{}/src/contrib/{}", self.url, self.package_version());
        let response = client.get(url).send().expect("TODO: handle get file error");

        if !response.status().is_success() {
            eprintln!("TODO: handle response failed: {}", response.status());
            process::exit(1);
        }
        response
    }

    #[cfg(not(target_os = "linux"))]
    fn get_file_url(&self, client: Client) -> Response {
        let url = "TODO: get different urls for each OS".to_string();
        let response = client.get(url).send().expect("TODO: handle get file error");

        if !response.status().is_success() {
            eprintln!("TODO: handle response failed: {}", response.status());
            process::exit(1);
        }
        response
    }

    fn package_version(&self) -> String {
        format!("{}_{}.tar.gz", self.name, self.version)
    }
}

mod tests {
    use super::*;

    #[test]
    fn can_download_tarball() {
        let download_path = Package::test_package().download("4.4.1");
        if let Ok(exists) = fs::exists(download_path) {
            assert!(exists)
        } else {
            panic!("Error")
        }
    }
}
