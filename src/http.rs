use std::{io::Write, time::Duration};
use reqwest::{blocking::{get, Client}, header::{HeaderMap, HeaderValue, USER_AGENT}};

// potentially generalize to use header arg instead of "user_agent" only
pub fn download<W: Write> (url: &str, mut writer: W, user_agent: Option<&str>) {

    let response = get_response(user_agent, url)
        .expect("TODO: handle response error");

    if !response.status().is_success() {
        panic!("TODO: handle url response is not success error")
    }

    let content = response.bytes()
        .expect("TODO: url response can't be converted to bytes");

    writer.write_all(&content)
        .expect("TODO: writer can't accept content");

    writer.flush().expect("TODO: writer can't be flushed");
}

fn get_response(user_agent: Option<&str>, url: &str) -> Result<reqwest::blocking::Response, reqwest::Error> {
    match user_agent {
        Some(user_agent) => {
            let mut headers = HeaderMap::new();
            headers.insert(USER_AGENT, 
                HeaderValue::try_from(user_agent).expect("TODO: handle header insert error"));
        
            let client = Client::builder()
                .default_headers(headers)
                .timeout(Duration::from_secs(20))
                .build()
                .expect("TODO: handle client build error");
        
            client.get(url).send()
        }
        None => {
            get(url)
        }
    }
}

#[allow(unused_imports)]
mod tests {
    use super::*;
    use std::fs::{self, File};

    #[test]
    fn can_download() {
        let url = "https://a2-ai.github.io/gh-pkg-mirror/2024-12-04/src/contrib/PACKAGES";
        let user_agent = None;
        let path = "/cluster-data/user-homes/wes/projects/rv/src/tests/http_downloader/PACKAGES.txt";
        let writer = File::create(&path).unwrap();
        let pre_size = fs::metadata(&path).unwrap().len();
        download(url, writer, user_agent);
        let post_size = fs::metadata(&path).unwrap().len();
        fs::remove_file(path).unwrap();
        assert!(pre_size < post_size);
    }
}