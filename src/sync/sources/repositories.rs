//! Download and install packages from repositories like CRAN, posit etc

use fs_err as fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use url::Url;

use crate::cache::{Cache, InstallationStatus};
use crate::consts::BUILT_FROM_SOURCE_FILENAME;
use crate::http::Http;
use crate::package::PackageType;
use crate::repository_urls::TarballUrls;
use crate::sync::LinkMode;
use crate::sync::errors::{SyncError, SyncErrorKind};
use crate::{
    Cancellation, HttpDownload, PackagePaths, RCmd, ResolvedDependency, get_tarball_urls,
    is_binary_package,
};

pub(crate) fn install_package(
    pkg: &ResolvedDependency,
    library_dirs: &[&Path],
    cache: &Cache,
    r_cmd: &impl RCmd,
    configure_args: &[String],
    cancellation: Arc<Cancellation>,
) -> Result<(), SyncError> {
    let (local_paths, global_paths) =
        cache.get_package_paths(&pkg.source, Some(&pkg.name), Some(&pkg.version.original));

    let compile_package = || -> Result<(), SyncError> {
        let source_path = local_paths.source.join(pkg.name.as_ref());
        log::debug!("Compiling package from {}", source_path.display());
        match r_cmd.install(
            &source_path,
            Option::<&Path>::None,
            library_dirs,
            &local_paths.binary,
            cancellation.clone(),
            &pkg.env_vars,
            configure_args,
        ) {
            Ok(output) => {
                // not using the path for the cache
                let log_path = cache.local().get_build_log_path(
                    &pkg.source,
                    Some(pkg.name.as_ref()),
                    Some(&pkg.version.original),
                );
                if let Some(parent) = log_path.parent() {
                    fs::create_dir_all(parent)?;
                    let mut f = fs::File::create(log_path)?;
                    f.write_all(output.as_bytes())?;
                }
                // Create the marker file for local compilation
                let _ = fs::File::create(
                    local_paths
                        .binary
                        .join(pkg.name.as_ref())
                        .join(BUILT_FROM_SOURCE_FILENAME),
                )?;
                Ok(())
            }
            Err(e) => Err(e.into()),
        }
    };

    // If the binary is not available, check the status to either download and/or compile
    if !pkg.cache_status.binary_available() {
        match pkg.cache_status.local {
            InstallationStatus::Source => {
                log::debug!(
                    "Package {} ({}) already present in cache as source but not as binary.",
                    pkg.name,
                    pkg.version.original
                );
                compile_package()?;
            }
            InstallationStatus::Absent => {
                log::debug!(
                    "Package {} ({}) not found in cache, trying to download it.",
                    pkg.name,
                    pkg.version.original
                );

                let tarball_url = get_tarball_urls(pkg, cache.r_version(), cache.system_info())
                    .expect("Dependency has source Repository");
                let http = Http {};

                if let PackageType::Source = download_package(
                    &http,
                    &tarball_url,
                    &local_paths,
                    &pkg.name,
                    &pkg.kind,
                    pkg.force_source,
                )? {
                    compile_package()?;
                }
            }
            _ => {}
        }
    }

    // And then we always link the binary folder into the staging library
    LinkMode::link_files(
        None,
        &pkg.name,
        if pkg.cache_status.global_binary_available() {
            global_paths.unwrap().binary
        } else {
            local_paths.binary
        },
        library_dirs.first().unwrap(),
    )?;

    Ok(())
}

fn download_package(
    http: &impl HttpDownload,
    urls: &TarballUrls,
    local_paths: &PackagePaths,
    pkg_name: &str,
    pkg_type: &PackageType,
    force_source: bool,
) -> Result<PackageType, SyncError> {
    // 1. Download Binary if possible/requested
    if let Some(binary_url) = &urls.binary
        && pkg_type == &PackageType::Binary
    {
        match try_download_package(http, binary_url, &local_paths, pkg_name, true) {
            Ok(pkg_type) => return Ok(pkg_type),
            Err(e) => {
                log::warn!(
                    "Failed to download binary from {}: {}. Trying binary archive",
                    binary_url,
                    e,
                );
            }
        }
    }

    // 2. Download binary from archive
    if let Some(binary_archive_url) = &urls.binary_archive
        && !force_source
    {
        match try_download_package(http, binary_archive_url, &local_paths, pkg_name, true) {
            Ok(pkg_type) => return Ok(pkg_type),
            Err(e) => {
                log::warn!(
                    "Failed to download binary archive from {}: {}. Trying source",
                    binary_archive_url,
                    e
                );
            }
        }
    }

    // 3. Download Source
    match try_download_package(http, &urls.source, &local_paths, pkg_name, false) {
        Ok(pkg_type) => return Ok(pkg_type),
        Err(e) => {
            log::warn!(
                "Failed to download source from {}: {}. Trying archive",
                &urls.source,
                e,
            );
        }
    }

    // 4. Download source from archive
    try_download_package(http, &urls.source_archive, &local_paths, pkg_name, false)
}

fn try_download_package(
    http: &impl HttpDownload,
    url: &Url,
    local_paths: &PackagePaths,
    pkg_name: &str,
    expect_binary: bool,
) -> Result<PackageType, SyncError> {
    let mut pkg_type = PackageType::Source;

    let destination = if expect_binary {
        &local_paths.binary
    } else {
        &local_paths.source
    };

    http.download_and_untar(url, destination, false, None)?;

    if expect_binary {
        let pkg_path = destination.join(pkg_name);
        if !is_binary_package(&pkg_path, pkg_name).map_err(|e| SyncError {
            source: SyncErrorKind::InvalidPackage {
                path: pkg_path,
                error: e.to_string(),
            },
        })? {
            log::debug!("{} was expected as binary, found to be source", pkg_name);
            // Move it to the source destination if we don't have it already
            if local_paths.source.is_dir() {
                fs::remove_dir_all(&local_paths.binary)?;
            } else {
                fs::create_dir_all(&local_paths.source)?;
                fs::rename(&local_paths.binary, &local_paths.source)?;
            }
        } else {
            pkg_type = PackageType::Binary;
        }
    }

    Ok(pkg_type)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpError, HttpErrorKind};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Mock HttpDownload that creates minimal package structures
    #[derive(Debug)]
    struct MockHttpDownload {
        /// Map of URL to Result - if URL is not in map, returns 404
        responses: Mutex<HashMap<String, Result<bool, u16>>>,
        /// Track which URLs were attempted in order
        attempts: Mutex<Vec<String>>,
    }

    impl MockHttpDownload {
        fn new() -> Self {
            Self {
                responses: Mutex::new(HashMap::new()),
                attempts: Mutex::new(Vec::new()),
            }
        }

        fn set_success(&self, url: &str, is_binary: bool) {
            self.responses
                .lock()
                .unwrap()
                .insert(url.to_string(), Ok(is_binary));
        }

        fn set_error(&self, url: &str) {
            self.responses
                .lock()
                .unwrap()
                .insert(url.to_string(), Err(404));
        }

        fn get_attempts(&self) -> Vec<String> {
            self.attempts.lock().unwrap().clone()
        }
    }

    impl HttpDownload for MockHttpDownload {
        fn download<W: std::io::Write>(
            &self,
            _url: &Url,
            _writer: &mut W,
            _headers: Vec<(&str, String)>,
        ) -> Result<u64, HttpError> {
            unimplemented!("Not used in these tests")
        }

        fn download_and_untar(
            &self,
            url: &Url,
            destination: impl AsRef<Path>,
            _use_sha_in_path: bool,
            _save_tarball_to: Option<&Path>,
        ) -> Result<(Option<PathBuf>, String), HttpError> {
            let destination = destination.as_ref();

            let url_str = url.to_string();
            self.attempts.lock().unwrap().push(url_str.clone());

            let responses = self.responses.lock().unwrap();
            match responses.get(&url_str) {
                Some(Ok(is_binary)) => {
                    let pkg_name = "testpkg";
                    if *is_binary {
                        create_binary_package(destination, pkg_name)
                            .map_err(|e| HttpError::from_io(&url_str, e))?;
                    } else {
                        create_source_package(destination, pkg_name)
                            .map_err(|e| HttpError::from_io(&url_str, e))?;
                    }
                    Ok((None, "fake_hash".to_string()))
                }
                Some(Err(e)) => Err(HttpError {
                    url: url_str,
                    source: HttpErrorKind::Http(*e),
                }),
                None => Err(HttpError {
                    url: url_str,
                    source: HttpErrorKind::Http(404),
                }),
            }
        }
    }

    fn create_binary_package(destination: impl AsRef<Path>, pkg_name: &str) -> std::io::Result<()> {
        let destination = destination.as_ref();
        let pkg_dir = destination.join(pkg_name);
        fs::create_dir_all(&pkg_dir)?;

        let desc_content = format!(
            "Package: {pkg_name}\nVersion: 1.0.0\nBuilt: R 4.5.0; ; 2025-01-01 00:00:00 UTC; unix\n"
        );

        fs::write(pkg_dir.join("DESCRIPTION"), desc_content)?;

        let meta_dir = pkg_dir.join("Meta");
        fs::create_dir_all(&meta_dir)?;
        fs::write(meta_dir.join("package.rds"), b"mock rds")?;

        Ok(())
    }

    fn create_source_package(destination: impl AsRef<Path>, pkg_name: &str) -> std::io::Result<()> {
        let destination = destination.as_ref();
        let pkg_dir = destination.join(pkg_name);
        fs::create_dir_all(&pkg_dir)?;

        let desc_content = format!("Package: {pkg_name}\nVersion: 1.0.0\n");

        fs::write(pkg_dir.join("DESCRIPTION"), desc_content)?;

        Ok(())
    }

    fn create_test_urls() -> TarballUrls {
        TarballUrls {
            binary: Some(Url::parse("https://example.com/binary.tar.gz").unwrap()),
            source: Url::parse("https://example.com/source.tar.gz").unwrap(),
            binary_archive: Some(Url::parse("https://example.com/archive/binary.tar.gz").unwrap()),
            source_archive: Url::parse("https://example.com/archive/source.tar.gz").unwrap(),
        }
    }

    fn create_test_paths() -> PackagePaths {
        let base = std::env::temp_dir()
            .join("rv_test_download_package")
            .join(format!(
                "test_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        PackagePaths {
            binary: base.join("binary"),
            source: base.join("source"),
        }
    }

    #[test]
    fn test_binary_succeeds_immediately() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // Binary URL succeeds with binary package
        mock.set_success("https://example.com/binary.tar.gz", true);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Binary, false);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PackageType::Binary);

        let attempts = mock.get_attempts();
        assert_eq!(attempts.len(), 1, "Should only try binary URL");
        assert_eq!(attempts[0], "https://example.com/binary.tar.gz");
    }

    #[test]
    fn test_binary_fails_fallback_to_source() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // Binary fails with 404, source succeeds
        mock.set_error("https://example.com/binary.tar.gz");
        mock.set_success("https://example.com/source.tar.gz", false);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Binary, false);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PackageType::Source);

        let attempts = mock.get_attempts();
        assert_eq!(attempts.len(), 2, "Should try binary, then source");
        assert_eq!(attempts[0], "https://example.com/binary.tar.gz");
        assert_eq!(attempts[1], "https://example.com/source.tar.gz");
    }

    #[test]
    fn test_binary_and_source_fail_fallback_to_binary_archive() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // Binary and source fail, binary_archive succeeds
        mock.set_error("https://example.com/binary.tar.gz");
        mock.set_error("https://example.com/source.tar.gz");
        mock.set_success("https://example.com/archive/binary.tar.gz", true);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Binary, false);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PackageType::Binary);

        let attempts = mock.get_attempts();
        assert_eq!(
            attempts.len(),
            3,
            "Should try binary, source, then binary_archive"
        );
        assert_eq!(attempts[0], "https://example.com/binary.tar.gz");
        assert_eq!(attempts[1], "https://example.com/source.tar.gz");
        assert_eq!(attempts[2], "https://example.com/archive/binary.tar.gz");
    }

    #[test]
    fn test_all_fail_except_source_archive() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // All fail except source_archive
        mock.set_error("https://example.com/binary.tar.gz");
        mock.set_error("https://example.com/source.tar.gz");
        mock.set_error("https://example.com/archive/binary.tar.gz");
        mock.set_success("https://example.com/archive/source.tar.gz", false);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Binary, false);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PackageType::Source);

        let attempts = mock.get_attempts();
        assert_eq!(attempts.len(), 4, "Should try all 4 URLs");
        assert_eq!(attempts[0], "https://example.com/binary.tar.gz");
        assert_eq!(attempts[1], "https://example.com/source.tar.gz");
        assert_eq!(attempts[2], "https://example.com/archive/binary.tar.gz");
        assert_eq!(attempts[3], "https://example.com/archive/source.tar.gz");
    }

    #[test]
    fn test_all_urls_fail() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // All URLs fail
        mock.set_error("https://example.com/binary.tar.gz");
        mock.set_error("https://example.com/source.tar.gz");
        mock.set_error("https://example.com/archive/binary.tar.gz");
        mock.set_error("https://example.com/archive/source.tar.gz");

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Binary, false);

        assert!(result.is_err());

        let attempts = mock.get_attempts();
        assert_eq!(attempts.len(), 4, "Should try all 4 URLs before failing");
    }

    #[test]
    fn test_source_package_skips_binary() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // Source succeeds
        mock.set_success("https://example.com/source.tar.gz", false);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Source, false);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), PackageType::Source);

        let attempts = mock.get_attempts();
        assert_eq!(
            attempts.len(),
            1,
            "Should skip binary and go straight to source"
        );
        assert_eq!(attempts[0], "https://example.com/source.tar.gz");
    }

    #[test]
    fn test_source_package_fallback_to_archive() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // Source fails, source_archive succeeds
        mock.set_error("https://example.com/source.tar.gz");
        mock.set_error("https://example.com/archive/binary.tar.gz");
        mock.set_success("https://example.com/archive/source.tar.gz", false);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Source, false);

        assert!(result.is_ok());

        let attempts = mock.get_attempts();
        assert_eq!(
            attempts.len(),
            3,
            "Should try source, binary_archive, then succeed on source_archive"
        );
        assert_eq!(attempts[0], "https://example.com/source.tar.gz");
        assert_eq!(attempts[1], "https://example.com/archive/binary.tar.gz");
        assert_eq!(attempts[2], "https://example.com/archive/source.tar.gz");
    }

    #[test]
    fn test_force_source_skips_binary_archive() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // Source fails, source archive succeeds
        mock.set_error("https://example.com/source.tar.gz");
        mock.set_success("https://example.com/archive/source.tar.gz", false);

        let result = download_package(
            &mock,
            &urls,
            &paths,
            "testpkg",
            &PackageType::Source,
            true, // force_source = true
        );

        assert!(result.is_ok());

        let attempts = mock.get_attempts();
        assert_eq!(attempts.len(), 2, "Should skip binary archive");
        assert_eq!(attempts[0], "https://example.com/source.tar.gz");
        assert_eq!(attempts[1], "https://example.com/archive/source.tar.gz");
    }

    #[test]
    fn test_no_force_source_gets_binary_archive_for_source() {
        // PackageType is source when pkg not found in the binary repository db
        // This should skip binary, fail for source, and succeed on binary archive
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // source fails, binary archive succeeds
        mock.set_error("https://example.com/source.tar.gz");
        mock.set_success("https://example.com/archive/binary.tar.gz", true);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Source, false);
        assert!(result.is_ok());

        let attempts = mock.get_attempts();
        assert_eq!(attempts.len(), 2, "Should hit binary archive");
        assert_eq!(attempts[0], "https://example.com/source.tar.gz");
        assert_eq!(attempts[1], "https://example.com/archive/binary.tar.gz");
    }

    #[test]
    fn test_binary_returns_source_package() {
        let mock = MockHttpDownload::new();
        let urls = create_test_urls();
        let paths = create_test_paths();

        // Binary URL returns source package (not compiled)
        mock.set_success("https://example.com/binary.tar.gz", false);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Binary, false);

        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            PackageType::Source,
            "Should detect binary is actually source"
        );

        let attempts = mock.get_attempts();
        assert_eq!(attempts.len(), 1, "Should only try binary URL");
        assert_eq!(attempts[0], "https://example.com/binary.tar.gz");
    }

    #[test]
    fn test_no_binary_url_skips_to_source() {
        let mock = MockHttpDownload::new();
        let mut urls = create_test_urls();
        urls.binary = None;
        let paths = create_test_paths();

        mock.set_success("https://example.com/source.tar.gz", false);

        let result = download_package(&mock, &urls, &paths, "testpkg", &PackageType::Binary, false);

        assert!(result.is_ok());

        let attempts = mock.get_attempts();
        assert_eq!(
            attempts.len(),
            1,
            "Should skip binary (not available) and go to source"
        );
        assert_eq!(attempts[0], "https://example.com/source.tar.gz");
    }
}
