use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{fs, io, io::Write, time::Duration};

use ureq::Agent;
use ureq::http::{HeaderName, HeaderValue};
use ureq::tls::{RootCerts, TlsConfig};
use url::Url;

use crate::fs::{copy_folder, untar_archive};

pub fn get_agent() -> Agent {
    Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .timeout_global(Some(Duration::from_secs(200)))
        .build()
        .new_agent()
}

/// Downloads a remote content to the given writer.
/// Returns the number of bytes written to the writer, 0 for a 404 or an empty 200
pub fn download<W: Write>(
    url: &Url,
    writer: &mut W,
    headers: Vec<(&str, String)>,
) -> Result<u64, HttpError> {
    let agent = get_agent();

    let mut request_builder = agent.get(url.as_str());

    {
        let req_headers = request_builder.headers_mut().unwrap();
        for (key, val) in headers {
            req_headers.insert(
                HeaderName::from_bytes(key.as_bytes()).unwrap(),
                HeaderValue::from_str(val.as_str()).expect("Invalid header value"),
            );
        }
    }
    log::trace!("Starting download of file from {url}");
    let start_time = Instant::now();

    match request_builder.call() {
        Ok(mut res) => {
            let mut reader = BufReader::new(res.body_mut().with_config().reader());
            let out = std::io::copy(&mut reader, writer).map_err(|e| HttpError {
                url: url.to_string(),
                source: HttpErrorKind::Io(e),
            });
            log::debug!(
                "Downloaded from {url} in {}ms",
                start_time.elapsed().as_millis()
            );
            out
        }
        Err(e) => {
            match e {
                // if the server returns an actual status code, we can get the response
                // to the later matcher
                ureq::Error::StatusCode(code) => Err(HttpError {
                    url: url.to_string(),
                    source: HttpErrorKind::Http(code),
                }),
                _ => Err(HttpError {
                    url: url.to_string(),
                    source: HttpErrorKind::Ureq(Box::new(e)),
                }),
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to download file from `{url}`")]
#[non_exhaustive]
pub struct HttpError {
    pub url: String,
    pub source: HttpErrorKind,
}

impl HttpError {
    pub(crate) fn from_io(url: &str, e: io::Error) -> Self {
        Self {
            url: url.to_string(),
            source: HttpErrorKind::Io(e),
        }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self.source, HttpErrorKind::Http(404))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpErrorKind {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Ureq(#[from] Box<ureq::Error>),
    #[error("Nothing found at URL")]
    Empty,
    #[error("File was found but could not be downloaded")]
    CantDownload,
    #[error("HTTP error code: {0}")]
    Http(u16),
}

pub trait HttpDownload {
    /// Downloads a file to the given writer and returns how many bytes were read
    fn download<W: Write>(
        &self,
        url: &Url,
        writer: &mut W,
        headers: Vec<(&str, String)>,
    ) -> Result<u64, HttpError>;

    /// Downloads what it meant to be a tarball and extract it at the given destination
    /// Returns the path where the files are if it's nested in a folder and the SHA256 hash of the tarball
    fn download_and_untar(
        &self,
        url: &Url,
        destination: impl AsRef<Path>,
        use_sha_in_path: bool,
    ) -> Result<(Option<PathBuf>, String), HttpError>;
}

pub struct Http;

impl HttpDownload for Http {
    fn download<W: Write>(
        &self,
        url: &Url,
        writer: &mut W,
        headers: Vec<(&str, String)>,
    ) -> Result<u64, HttpError> {
        let bytes_read = download(url, writer, headers)?;
        if bytes_read == 0 {
            Err(HttpError {
                url: url.to_string(),
                source: HttpErrorKind::Empty,
            })
        } else {
            Ok(bytes_read)
        }
    }

    fn download_and_untar(
        &self,
        url: &Url,
        destination: impl AsRef<Path>,
        use_sha_in_path: bool,
    ) -> Result<(Option<PathBuf>, String), HttpError> {
        let destination = destination.as_ref().to_path_buf();

        let mut writer = Vec::new();
        self.download(url, &mut writer, vec![])?;

        let start = Instant::now();

        let (destination, dir, sha) = if use_sha_in_path {
            // If we want to use the sha in path, we need to untar first so we get the sha rather
            // than reading the file twice
            let tempdir = tempfile::tempdir().map_err(|e| HttpError::from_io(url.as_str(), e))?;
            let (dir, sha) = untar_archive(Cursor::new(writer), tempdir.path(), true)
                .map_err(|e| HttpError::from_io(url.as_str(), e))?;
            let actual_dir = dir.unwrap();
            let sha = sha.unwrap();
            let new_destination = destination.join(&sha[..10]);
            let install_dir = new_destination.join(actual_dir.file_name().unwrap());
            if install_dir.is_dir() {
                fs::remove_dir_all(&install_dir)
                    .map_err(|e| HttpError::from_io(url.as_str(), e))?;
            }
            fs::create_dir_all(&install_dir).map_err(|e| HttpError::from_io(url.as_str(), e))?;
            copy_folder(&actual_dir, &install_dir)
                .map_err(|e| HttpError::from_io(url.as_str(), e))?;

            (new_destination, Some(install_dir), sha)
        } else {
            let (dir, sha) = untar_archive(Cursor::new(writer), &destination, true)
                .map_err(|e| HttpError::from_io(url.as_str(), e))?;
            (destination, dir, sha.unwrap())
        };

        log::debug!(
            "Successfully extracted archive to {} (in sub folder: {:?}) in {}ms",
            destination.display(),
            dir,
            Instant::now().duration_since(start).as_millis()
        );

        Ok((dir, sha))
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

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

        let result = super::download(&Url::parse(&url).unwrap(), &mut writer, Vec::new());
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

        let result = super::download(&Url::parse(&url).unwrap(), &mut writer, headers);
        assert!(result.is_ok());
        mock_endpoint.assert();
        assert_eq!(writer.into_inner(), b"Mock file content".to_vec());
    }
}
