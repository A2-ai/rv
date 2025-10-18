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

/// This should only be run on Linux
pub fn get_system_requirements(system_info: &SystemInfo) -> HashMap<String, Vec<String>> {
    let agent = http::get_agent();
    let mut url = Url::parse(&get_sysreq_url()).unwrap();

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
        .unwrap()
        .body_mut()
        .read_json::<Response>()
        .unwrap();

    let mut out = HashMap::new();
    for package in response.requirements {
        out.insert(package.name, package.requirements.packages);
    }

    out
}

/// Extract package name from rpm query output
/// Input: "bash-4.4.20-6.el8_10.x86_64"
/// Output: Some("bash")
///
/// RPM package naming: name-version-release.arch
/// We need to split on the first hyphen that's followed by a version number
fn extract_rpm_package_name(rpm_output: &str) -> Option<&str> {
    // Find the first hyphen followed by a digit (start of version)
    let mut last_name_idx = 0;
    let chars: Vec<char> = rpm_output.chars().collect();

    for i in 0..chars.len().saturating_sub(1) {
        if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
            last_name_idx = i;
            break;
        }
    }

    if last_name_idx > 0 {
        Some(&rpm_output[..last_name_idx])
    } else {
        // Fallback: if no version found, might be just a package name
        if !rpm_output.contains('-') {
            Some(rpm_output)
        } else {
            None
        }
    }
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
        }

        "centos" | "redhat" | "rockylinux" | "opensuse" | "sle" => {
            // Running rpm -q {..pkg_list} and parse stdout
            let command = Command::new("rpm")
                .arg("-q")
                .args(sys_deps)
                .output()
                .expect("to be able to run rpm command");

            let stdout = String::from_utf8(command.stdout).unwrap();
            let stderr = String::from_utf8(command.stderr).unwrap();

            // Parse stdout for installed packages
            // Format: "packagename-version-release.arch"
            for line in stdout.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    // Extract package name (everything before first hyphen followed by a digit)
                    if let Some(pkg_name) = extract_rpm_package_name(line) {
                        if let Some(status) = out.get_mut(pkg_name) {
                            *status = SysInstallationStatus::Present;
                        }
                    }
                }
            }

            // Also check stderr to see if any packages printed "not installed" messages
            // This helps us mark things as definitively Absent vs Unknown
            for line in stderr.lines() {
                // Format: "package NAME is not installed"
                if line.contains("is not installed") {
                    if let Some(pkg_name) = line.split_whitespace().nth(1) {
                        if let Some(status) = out.get_mut(pkg_name) {
                            if status == &SysInstallationStatus::Unknown {
                                *status = SysInstallationStatus::Absent;
                            }
                        }
                    }
                }
            }

            // Check PATH for known tools (same as dpkg logic)
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
        }

        _ => (),
    };

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

    #[test]
    fn test_extract_rpm_package_name() {
        assert_eq!(
            extract_rpm_package_name("bash-4.4.20-6.el8_10.x86_64"),
            Some("bash")
        );
        assert_eq!(
            extract_rpm_package_name("libcurl-devel-7.61.1-34.el8_10.8.x86_64"),
            Some("libcurl-devel")
        );
        assert_eq!(
            extract_rpm_package_name("abseil-cpp-devel-20210324.2-1.el8.x86_64"),
            Some("abseil-cpp-devel")
        );
        assert_eq!(extract_rpm_package_name("bash"), Some("bash"));
        assert_eq!(
            extract_rpm_package_name("openssl-devel-1.1.1k-14.el8_6.x86_64"),
            Some("openssl-devel")
        );
    }

    #[test]
    fn test_alma8_is_supported() {
        let system = SystemInfo::new(
            OsType::Linux("almalinux"),
            Some("x86_64".to_string()),
            None,
            "8.10",
        );
        assert!(is_supported(&system));
    }

    #[test]
    fn test_alma9_is_supported() {
        let system = SystemInfo::new(
            OsType::Linux("almalinux"),
            Some("x86_64".to_string()),
            None,
            "9.0",
        );
        assert!(is_supported(&system));
    }

    #[test]
    fn test_alma8_api_mapping() {
        let system = SystemInfo::new(
            OsType::Linux("almalinux"),
            Some("x86_64".to_string()),
            None,
            "8.10",
        );
        let (distrib, version) = system.sysreq_data();
        assert_eq!(distrib, "centos");
        assert_eq!(version, "8"); // Major version only for RPM distros
    }

    #[test]
    fn test_alma9_api_mapping() {
        let system = SystemInfo::new(
            OsType::Linux("almalinux"),
            Some("x86_64".to_string()),
            None,
            "9.0",
        );
        let (distrib, version) = system.sysreq_data();
        assert_eq!(distrib, "rockylinux");
        assert_eq!(version, "9"); // Major version only for RPM distros
    }

    #[test]
    fn test_centos9_api_mapping() {
        let system = SystemInfo::new(
            OsType::Linux("centos"),
            Some("x86_64".to_string()),
            None,
            "9.0",
        );
        let (distrib, version) = system.sysreq_data();
        assert_eq!(distrib, "rockylinux");
        assert_eq!(version, "9"); // Major version only for RPM distros
    }

    #[test]
    fn test_centos8_api_mapping() {
        let system = SystemInfo::new(
            OsType::Linux("centos"),
            Some("x86_64".to_string()),
            None,
            "8.5",
        );
        let (distrib, version) = system.sysreq_data();
        assert_eq!(distrib, "centos");
        assert_eq!(version, "8"); // Major version only for RPM distros
    }
}
