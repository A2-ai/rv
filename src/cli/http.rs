use std::{io::Write, time::Duration};

use anyhow::{bail, Context, Result};

/// Downloads a remote content to the given writer.
/// Returns the number of bytes written to the writer, 0 for a 404 or an empty 200
pub fn download<W: Write>(url: &str, writer: &mut W, headers: Vec<(&str, String)>) -> Result<u64> {
    let mut request = ureq::get(url).timeout(Duration::from_secs(20));
    for (key, val) in headers {
        request = request.set(key, &val);
    }
    let ogresp = request
        .call();
    
    let resp = match ogresp {
        Ok(r) => r,
        Err(e) => {
            match e {
                // if the server returns an actual status code, we can get the response
                // to the later matcher
                ureq::Error::Status(_, resp) => resp ,
                _ => bail!("Error downloading file from: {url}: {e}"),
            }
        }
    };
    // in practice an empty 200 and a 404 will be treated the same
    match resp.status() {
        200 => std::io::copy(&mut resp.into_reader(), writer)
            .with_context(|| format!("File at {url} was found but could not be downloaded.")),
        404 => Ok(0),
        _ => bail!(
            "Unexpected HTTP error when downloading file {url} [{}]: {}",
            resp.status(),
            resp.into_string()?
        ),
    }
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

        let result = super::download(&url, &mut writer, Vec::new());
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
        let headers = vec![("custom-header", "custom-value".to_string())];

        let result = super::download(&url, &mut writer, headers);
        assert!(result.is_ok());
        mock_endpoint.assert();
        assert_eq!(writer.into_inner(), b"Mock file content".to_vec());
    }
}
