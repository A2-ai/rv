use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{io, io::Write, time::Duration};

use crate::fs::untar_archive;
use sha2::{Digest, Sha256};

// A writer that returns the sha256 hash at the end
struct ShaWriter<W: Write> {
    inner: W,
    hasher: Sha256,
}

impl<W: Write> ShaWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    #[must_use]
    fn finish(self) -> (W, String) {
        let hash = self.hasher.finalize();
        (self.inner, format!("{hash:x}"))
    }
}

impl<W: Write> Write for ShaWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Downloads a remote content to the given writer.
/// Returns the number of bytes written to the writer, 0 for a 404 or an empty 200
pub fn download<W: Write>(
    url: &str,
    writer: &mut W,
    headers: Vec<(&str, String)>,
) -> Result<u64, HttpError> {
    let mut request = ureq::get(url).timeout(Duration::from_secs(200));
    for (key, val) in headers {
        request = request.set(key, &val);
    }
    log::trace!("Starting download of file from {url}");
    let start_time = Instant::now();
    let og_resp = request.call();

    let resp = match og_resp {
        Ok(r) => r,
        Err(e) => {
            match e {
                // if the server returns an actual status code, we can get the response
                // to the later matcher
                ureq::Error::Status(_, resp) => resp,
                _ => {
                    return Err(HttpError {
                        url: url.to_string(),
                        source: HttpErrorKind::Ureq(Box::new(e)),
                    })
                }
            }
        }
    };

    match resp.status() {
        200 => {
            let out = std::io::copy(&mut resp.into_reader(), writer).map_err(|e| HttpError {
                url: url.to_string(),
                source: HttpErrorKind::Io(e),
            });
            log::debug!(
                "Downloaded from {url} in {}ms",
                start_time.elapsed().as_millis()
            );
            out
        }
        _ => Err(HttpError {
            url: url.to_string(),
            source: HttpErrorKind::Http(resp.status()),
        }),
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
    fn from_io(url: &str, e: io::Error) -> Self {
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
        url: &str,
        writer: &mut W,
        headers: Vec<(&str, String)>,
    ) -> Result<u64, HttpError>;

    /// Downloads what it meant to be a tarball and extract it at the given destination
    /// Returns the path where the files are if it's nested in a folder and the SHA256 hash of the tarball
    fn download_and_untar(
        &self,
        url: &str,
        destination: impl AsRef<Path>,
        use_sha_in_path: bool,
    ) -> Result<(Option<PathBuf>, String), HttpError>;
}

pub struct Http;

impl HttpDownload for Http {
    fn download<W: Write>(
        &self,
        url: &str,
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
        url: &str,
        destination: impl AsRef<Path>,
        use_sha_in_path: bool,
    ) -> Result<(Option<PathBuf>, String), HttpError> {
        let destination = destination.as_ref();

        let mut writer = ShaWriter::new(Vec::new());
        self.download(url, &mut writer, vec![])?;
        let (inner, sha) = writer.finish();

        let final_dest = if use_sha_in_path {
            destination.join(&sha[..10])
        } else {
            destination.to_path_buf()
        };

        let dir = untar_archive(Cursor::new(inner), &final_dest)
            .map_err(|e| HttpError::from_io(url, e))?;

        log::debug!(
            "Successfully extracted archive to {} (in sub folder: {:?})",
            final_dest.display(),
            dir
        );

        Ok((dir, sha))
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
