use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::time::Instant;
use std::{fs, io, io::Write, time::Duration};

use ureq::Agent;
use ureq::http::{HeaderName, HeaderValue};
use ureq::tls::{RootCerts, TlsConfig};
use url::Url;

use crate::consts::INSECURE_TLS_ENV_VAR_NAME;
use crate::fs::{copy_folder, untar_archive};
use crate::utils::is_env_var_truthy;

static INSECURE_WARNING: Once = Once::new();

pub(crate) fn build_agent(insecure: bool) -> Agent {
    let mut tls_builder = TlsConfig::builder().root_certs(RootCerts::PlatformVerifier);
    if insecure {
        tls_builder = tls_builder.disable_verification(true);
    }
    Agent::config_builder()
        .tls_config(tls_builder.build())
        .timeout_global(Some(Duration::from_secs(200)))
        .build()
        .new_agent()
}

pub fn get_agent() -> Agent {
    let insecure = is_env_var_truthy(INSECURE_TLS_ENV_VAR_NAME);
    if insecure {
        INSECURE_WARNING.call_once(|| {
            log::warn!("TLS certificate verification is disabled via {INSECURE_TLS_ENV_VAR_NAME}");
        });
    }
    build_agent(insecure)
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

/// Downloads a file from URL and saves it to the given path
pub fn download_to_file(url: &Url, path: &Path) -> Result<(), HttpError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| HttpError::from_io(url.as_str(), e))?;
    }
    let mut file = fs::File::create(path).map_err(|e| HttpError::from_io(url.as_str(), e))?;
    download(url, &mut file, vec![])?;
    Ok(())
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
    /// If `save_tarball_to` is Some, the raw tarball bytes will be written to that path
    fn download_and_untar(
        &self,
        url: &Url,
        destination: impl AsRef<Path>,
        use_sha_in_path: bool,
        save_tarball_to: Option<&Path>,
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
        save_tarball_to: Option<&Path>,
    ) -> Result<(Option<PathBuf>, String), HttpError> {
        let destination = destination.as_ref().to_path_buf();

        let mut writer = Vec::new();
        self.download(url, &mut writer, vec![])?;

        // Save tarball to disk if requested
        if let Some(tarball_path) = save_tarball_to {
            if let Some(parent) = tarball_path.parent() {
                fs::create_dir_all(parent).map_err(|e| HttpError::from_io(url.as_str(), e))?;
            }
            fs::write(tarball_path, &writer).map_err(|e| HttpError::from_io(url.as_str(), e))?;
        }

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

    /// Spin up a minimal HTTPS server on localhost with a self-signed certificate.
    /// Returns `(port, server_thread)`. The server handles exactly one request then exits.
    fn spawn_self_signed_https_server() -> (u16, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        use std::sync::Arc;

        let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let cert_der = rustls::pki_types::CertificateDer::from(cert.cert.der().to_vec());
        let key_der =
            rustls::pki_types::PrivateKeyDer::try_from(cert.signing_key.serialize_der()).unwrap();

        let server_config = Arc::new(
            rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert_der], key_der)
            .unwrap(),
        );

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut conn = rustls::ServerConnection::new(server_config).unwrap();
            let mut tls = rustls::Stream::new(&mut conn, &mut stream);

            let mut buf = [0u8; 1024];
            let _ = tls.read(&mut buf);

            let body = b"hello from self-signed server";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
            let _ = tls.write_all(response.as_bytes());
            let _ = tls.write_all(body);
            let _ = tls.flush();
        });

        (port, handle)
    }

    #[test]
    fn insecure_agent_skips_tls_verification() {
        let (port, server_handle) = spawn_self_signed_https_server();
        let url = format!("https://localhost:{port}/");

        // Verification disabled: should succeed against the self-signed cert
        let agent = super::build_agent(true);
        let resp = agent.get(&url).call();
        assert!(
            resp.is_ok(),
            "insecure agent should connect to self-signed server"
        );

        server_handle.join().unwrap();
    }

    #[test]
    fn secure_agent_rejects_self_signed_cert() {
        let (port, server_handle) = spawn_self_signed_https_server();
        let url = format!("https://localhost:{port}/");

        // Verification enabled (default): should fail against the self-signed cert
        let agent = super::build_agent(false);
        let resp = agent.get(&url).call();
        assert!(
            resp.is_err(),
            "secure agent should reject self-signed server"
        );

        // The server thread may error since the client disconnects during handshake — that's fine
        let _ = server_handle.join();
    }

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
