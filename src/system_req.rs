use crate::{SystemInfo, http};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fmt::Formatter;
use std::process::Command;

use serde::{Deserialize, Serialize};
use url::Url;
use which::which;

use crate::consts::{SYS_DEPS_CHECK_IN_PATH_ENV_VAR_NAME, SYS_REQ_URL_ENV_VAR_NAME};

/// https://rserver.tradecraftclinical.com/rspm/__api__/swagger/index.html#/default/get_repos__id__sysreqs
const SYSTEM_REQ_API_URL: &str = "https://packagemanager.posit.co/__api__/repos/cran/sysreqs";
/// Some tools might not be installed by the package manager
const KNOWN_THINGS_IN_PATH: &[&str] = &[
    "rustc",
    "cargo",
    "pandoc",
    "texlive",
    "chromium",
    "google-chrome",
];

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SysInstallationStatus {
    Present,
    Absent,
    Unknown,
}

impl fmt::Display for SysInstallationStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Present => write!(f, "present"),
            Self::Absent => write!(f, "absent"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SysDep {
    pub name: String,
    pub status: SysInstallationStatus,
}

impl SysDep {
    pub fn new(name: String) -> Self {
        Self {
            name,
            status: SysInstallationStatus::Unknown,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Requirements {
    // not all requirements have packages. Some are pre_/post_install
    #[serde(default)]
    packages: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Package {
    name: String,
    requirements: Requirements,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Response {
    requirements: Vec<Package>,
}

fn get_sysreq_url() -> String {
    std::env::var(SYS_REQ_URL_ENV_VAR_NAME).unwrap_or_else(|_| SYSTEM_REQ_API_URL.to_string())
}

pub fn is_supported(system_info: &SystemInfo) -> bool {
    let (distrib, version) = system_info.sysreq_data();

    match distrib {
        "ubuntu" => ["20.04", "22.04", "24.04"].contains(&version.as_str()),
        "debian" => version.starts_with("12"),
        "centos" => version.starts_with("7") || version.starts_with("8"),
        "redhat" => {
            version.starts_with("7") || version.starts_with("8") || version.starts_with("9")
        }
        "rockylinux" => version.starts_with("8") || version.starts_with("9"),
        "opensuse" | "sle" => version.starts_with("15"),
        _ => false,
    }
}

/// Errors that can happen while fetching or caching system requirements.
#[derive(Debug, thiserror::Error)]
pub enum SysReqError {
    #[error("Invalid system requirements URL `{url}` (check {SYS_REQ_URL_ENV_VAR_NAME})")]
    InvalidUrl {
        url: String,
        #[source]
        source: url::ParseError,
    },
    #[error("Failed to fetch system requirements from `{url}`")]
    Request {
        url: String,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("Failed to parse the system requirements response from `{url}`")]
    InvalidResponse {
        url: String,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("Failed to read or write the system requirements cache")]
    Cache(#[from] std::io::Error),
    #[error("Failed to (de)serialize the cached system requirements")]
    CacheParse(#[from] serde_json::Error),
}

/// Validates the `RV_SYS_REQ_URL` environment variable (when set) is a valid URL
pub fn validate_sysreq_url() -> Result<(), SysReqError> {
    if std::env::var(SYS_REQ_URL_ENV_VAR_NAME).is_ok() {
        let url = get_sysreq_url();
        Url::parse(&url).map_err(|source| SysReqError::InvalidUrl { url, source })?;
    }
    Ok(())
}

/// This should only be run on Linux
pub fn get_system_requirements(
    system_info: &SystemInfo,
) -> Result<HashMap<String, Vec<String>>, SysReqError> {
    fetch_system_requirements(&get_sysreq_url(), system_info)
}

fn fetch_system_requirements(
    base_url: &str,
    system_info: &SystemInfo,
) -> Result<HashMap<String, Vec<String>>, SysReqError> {
    let agent = http::get_agent();
    let mut url = Url::parse(base_url).map_err(|source| SysReqError::InvalidUrl {
        url: base_url.to_string(),
        source,
    })?;

    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("all", "true");
        let (distrib, version) = system_info.sysreq_data();
        pairs.append_pair("distribution", distrib);
        pairs.append_pair("release", version.as_str());
    }

    log::debug!("Getting sysreq data from {}", url.as_str());

    let response = agent
        .get(url.as_str())
        .header("Accept", "application/json")
        .call()
        .map_err(|source| SysReqError::Request {
            url: url.to_string(),
            source: Box::new(source),
        })?
        .body_mut()
        .read_json::<Response>()
        .map_err(|source| SysReqError::InvalidResponse {
            url: url.to_string(),
            source: Box::new(source),
        })?;

    let mut out = HashMap::new();
    for package in response.requirements {
        out.insert(package.name, package.requirements.packages);
    }

    Ok(out)
}

/// Extract package name from rpm query output
/// Input: "bash-4.4.20-6.el8_10.x86_64"
/// Output: Some("bash")
///
/// RPM package naming: name-version-release.arch
/// We need to split on the first hyphen that's followed by a version number
fn extract_rpm_package_name(rpm_output: &str) -> Option<&str> {
    let bytes = rpm_output.as_bytes();

    for i in 0..bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'-' && bytes[i + 1].is_ascii_digit() {
            return Some(&rpm_output[..i]);
        }
    }

    // No version pattern found, return the whole string
    Some(rpm_output)
}

pub fn check_installation_status(
    system_info: &SystemInfo,
    sys_deps: &HashSet<&str>,
) -> HashMap<String, SysInstallationStatus> {
    if !is_supported(system_info) {
        return HashMap::new();
    }

    let mut out = HashMap::from_iter(
        sys_deps
            .iter()
            .map(|x| (x.to_string(), SysInstallationStatus::Unknown)),
    );
    if sys_deps.is_empty() {
        return out;
    }

    log::debug!("Checking installation status for {:?}", sys_deps);
    let from_env = std::env::var(SYS_DEPS_CHECK_IN_PATH_ENV_VAR_NAME).unwrap_or_default();
    match system_info.sysreq_data().0 {
        "ubuntu" | "debian" => {
            // Running dpkg-query -W -f='${Package}\n' {..pkg_list} and read stdout
            let command = Command::new("dpkg-query")
                .arg("-W")
                .arg("-f=${Package}\n")
                .args(sys_deps)
                .output()
                .expect("to be able to run commands");

            let stdout = String::from_utf8(command.stdout).unwrap();
            for line in stdout.lines() {
                if let Some(status) = out.get_mut(line.trim()) {
                    *status = SysInstallationStatus::Present;
                }
            }
        }

        "centos" | "redhat" | "rockylinux" | "opensuse" | "sle" => {
            // Running rpm -q {..pkg_list} and parse stdout
            let command = match Command::new("rpm").arg("-q").args(sys_deps).output() {
                Ok(output) => output,
                Err(e) => {
                    log::warn!(
                        "Failed to run rpm command: {}. System dependencies detection skipped.",
                        e
                    );
                    return out;
                }
            };

            let stdout = String::from_utf8_lossy(&command.stdout);
            let stderr = String::from_utf8_lossy(&command.stderr);

            // Parse stdout for installed packages
            // Format: "packagename-version-release.arch"
            for line in stdout.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    // Extract package name (everything before first hyphen followed by a digit)
                    if let Some(pkg_name) = extract_rpm_package_name(line)
                        && let Some(status) = out.get_mut(pkg_name)
                    {
                        *status = SysInstallationStatus::Present;
                    }
                }
            }

            // Also check stderr to see if any packages printed "not installed" messages
            // This helps us mark things as definitively Absent vs Unknown
            for line in stderr.lines() {
                // Format: "package NAME is not installed"
                if line.contains("is not installed")
                    && let Some(pkg_name) = line.split_whitespace().nth(1)
                    && let Some(status) = out.get_mut(pkg_name)
                    && status == &SysInstallationStatus::Unknown
                {
                    *status = SysInstallationStatus::Absent;
                }
            }
        }

        _ => (),
    };

    let mut to_check_in_path: Vec<_> = from_env.split(",").map(|x| x.trim()).collect();
    to_check_in_path.extend_from_slice(KNOWN_THINGS_IN_PATH);

    for (name, status) in out
        .iter_mut()
        .filter(|(_, v)| v == &&SysInstallationStatus::Unknown)
    {
        if to_check_in_path.contains(&name.as_str()) {
            if which(name).is_ok() {
                *status = SysInstallationStatus::Present;
            } else {
                *status = SysInstallationStatus::Absent;
            }
        }
    }

    for (_, status) in out
        .iter_mut()
        .filter(|(_, x)| **x == SysInstallationStatus::Unknown)
    {
        *status = SysInstallationStatus::Absent;
    }

    out
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{OsType, SystemInfo};
    use std::fs;

    #[test]
    fn test_ubuntu_20_04() {
        let content = fs::read_to_string("src/tests/sys_reqs/ubuntu_20.04.json").unwrap();
        assert!(serde_json::from_str::<Response>(&content).is_ok());
    }

    fn linux_system_info() -> SystemInfo {
        SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            None,
            "22.04",
        )
    }

    #[test]
    fn fetch_returns_parsed_requirements() {
        let mut server = mockito::Server::new();
        let body = r#"{"requirements":[{"name":"libcurl","requirements":{"packages":["libcurl4-openssl-dev","libssl-dev"]}}]}"#;
        let mock = server
            .mock("GET", "/sysreqs")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(body)
            .create();

        let base_url = format!("{}/sysreqs", server.url());
        let result = fetch_system_requirements(&base_url, &linux_system_info());
        mock.assert();

        let map = result.expect("valid JSON should parse");
        assert_eq!(
            map.get("libcurl"),
            Some(&vec![
                "libcurl4-openssl-dev".to_string(),
                "libssl-dev".to_string()
            ])
        );
    }

    #[test]
    fn fetch_non_json_returns_error_not_panic() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/sysreqs")
            .match_query(mockito::Matcher::Any)
            .with_status(200)
            .with_header("Content-Type", "text/html")
            .with_body("<html>not json</html>")
            .create();

        let base_url = format!("{}/sysreqs", server.url());
        let result = fetch_system_requirements(&base_url, &linux_system_info());
        mock.assert();

        assert!(matches!(result, Err(SysReqError::InvalidResponse { .. })));
    }

    #[test]
    fn fetch_server_error_returns_error_not_panic() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/sysreqs")
            .match_query(mockito::Matcher::Any)
            .with_status(500)
            .create();

        let base_url = format!("{}/sysreqs", server.url());
        let result = fetch_system_requirements(&base_url, &linux_system_info());
        mock.assert();

        assert!(matches!(result, Err(SysReqError::Request { .. })));
    }

    #[test]
    fn fetch_invalid_url_returns_error_not_panic() {
        let result = fetch_system_requirements("not a valid url", &linux_system_info());
        assert!(matches!(result, Err(SysReqError::InvalidUrl { .. })));
    }

    #[test]
    fn test_extract_rpm_package_name() {
        let test_cases = vec![
            ("bash-4.4.20-6.el8_10.x86_64", Some("bash")),
            (
                "libcurl-devel-7.61.1-34.el8_10.8.x86_64",
                Some("libcurl-devel"),
            ),
            (
                "abseil-cpp-devel-20210324.2-1.el8.x86_64",
                Some("abseil-cpp-devel"),
            ),
            ("bash", Some("bash")),
            (
                "openssl-devel-1.1.1k-14.el8_6.x86_64",
                Some("openssl-devel"),
            ),
        ];

        for (input, expected) in test_cases {
            assert_eq!(
                extract_rpm_package_name(input),
                expected,
                "Failed for input: {}",
                input
            );
        }
    }

    #[test]
    fn test_is_supported() {
        let test_cases = vec![("almalinux", "8.10", true), ("almalinux", "9.0", true)];

        for (os_name, version, expected) in test_cases {
            let system = SystemInfo::new(
                OsType::Linux(os_name),
                Some("x86_64".to_string()),
                None,
                version,
            );
            assert_eq!(
                is_supported(&system),
                expected,
                "Failed for {} {}",
                os_name,
                version
            );
        }
    }

    #[test]
    fn test_api_mapping() {
        let test_cases = vec![
            ("almalinux", "8.10", "centos", "8"),
            ("almalinux", "9.0", "rockylinux", "9"),
            ("centos", "9.0", "rockylinux", "9"),
            ("centos", "8.5", "centos", "8"),
        ];

        for (os_name, version, expected_distrib, expected_version) in test_cases {
            let system = SystemInfo::new(
                OsType::Linux(os_name),
                Some("x86_64".to_string()),
                None,
                version,
            );
            let (distrib, version) = system.sysreq_data();
            assert_eq!(
                distrib, expected_distrib,
                "Failed distrib mapping for {} {}",
                os_name, version
            );
            assert_eq!(
                version, expected_version,
                "Failed version mapping for {} {}",
                os_name, version
            );
        }
    }
}
