use std::{io::Read, os::linux::raw::stat, time::Duration};

use reqwest::blocking::Client;

fn repo_path(url: String) {
    
}

fn api_status_url(url: String) -> Option<String>{
    let mut s: Vec<&str> = url.split("/").collect();
    while s.last().unwrap_or(&"") != &"" {
        let test_api_status_url = format!("{}/__api__/status", s.join(""));
        if test_api_status(test_api_status_url) { return Some(test_api_status_url); }
        s.pop();
    }
    None
}

fn test_api_status(status_url: String) -> Option<String> {
    if let Ok(mut response) = Client::new()
        .get(status_url)
        .timeout(Duration::from_millis(500))
        .send() {
        if !response.status().is_success() { return None }

        let mut content = String::new();
        response.read_to_string(&mut content).expect("msg");
    }
    return None;
}

mod tests {
    use super::*;

    #[test]
    fn tester() {
        repo_path("https://packagemanager.posit.co/cran/2024-10-06".to_string());
    }
}