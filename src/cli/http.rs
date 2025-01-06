use reqwest::blocking::Client;
use std::{io::Write, time::Duration};

// potentially generalize to use header arg instead of "user_agent" only
pub fn download<W: Write>(
    url: &str,
    writer: &mut W,
) -> Result<(), Box<dyn std::error::Error>> {
    let response = get_response(url).expect("TODO: handle response error");

    if !response.status().is_success() {
        return Err(format!("Failed to download: {}", response.status()).into());
    }

    let content = response
        .bytes()
        .expect("TODO: url response can't be converted to bytes");

    writer
        .write_all(&content)
        .expect("TODO: writer can't accept content");

    Ok(())
}

fn get_response(
    url: &str,
) -> Result<reqwest::blocking::Response, reqwest::Error> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .expect("TODO: handle client build error");

    client.get(url).send()
}

mod tests {
    #[test]
    fn mock_download_with_no_header() {
        let mut server = mockito::Server::new();
        let mock_url = server.url();
        let mock_endpoint = server
            .mock("GET", "/file.txt")
            .with_status(200)
            .with_header("Content-Type", "text/plain")
            .with_body("Mock file content")
            .create();

        let url = format!("{mock_url}/file.txt");
        let mut writer = std::io::Cursor::new(Vec::new());

        let result = super::download(&url, &mut writer);
        assert!(result.is_ok());
        mock_endpoint.assert();
        assert_eq!(writer.into_inner(), b"Mock file content".to_vec());
    }

    #[test]
    fn mock_download_with_header() {
        let mut server = mockito::Server::new();
        let mock_url = server.url();
        let mock_endpoint = server
            .mock("GET", "/file.txt")
            .with_status(200)
            .with_header("Content-Type", "text/plain")
            .with_body("Mock file content")
            .create();

        let url = format!("{mock_url}/file.txt");
        let mut writer = std::io::Cursor::new(Vec::new());

        let result = super::download(&url, &mut writer);
        assert!(result.is_ok());
        mock_endpoint.assert();
        assert_eq!(writer.into_inner(), b"Mock file content".to_vec());
    }
}
