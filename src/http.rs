use std::{io::Write, time::Duration};
use reqwest::{blocking::{get, Client}, header::{HeaderMap, HeaderValue, USER_AGENT}};

// potentially generalize to use header arg instead of "user_agent" only
pub fn download<W: Write> (url: &str, mut writer: W, user_agent: Option<&str>) {

    let response = get_response(url, user_agent)
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

fn get_response(url: &str, user_agent: Option<&str>) -> Result<reqwest::blocking::Response, reqwest::Error> {
    if let Some(ua) = user_agent {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, 
            HeaderValue::try_from(ua).expect("TODO: handle header insert error"));
    
        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(20))
            .build()
            .expect("TODO: handle client build error");
    
        return client.get(url).send();
    }
        
    get(url)
}