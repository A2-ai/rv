//! For R we will need some information on what is the current OS.
//! We can get that information from the `os_info` crate but we don't want to expose its type
//! to the library/CLI.
//! Instead, we encode the data we care about in an enum that can easily be shared

use os_info::{Type, Version};
use serde::Serialize;
use std::fmt;

/// For R we only care about Windows, MacOS and Linux
#[derive(Debug, PartialEq, Clone, Copy, Serialize)]
pub enum OsType {
    Windows,
    MacOs,
    Linux(&'static str),
    // TODO: we should error before we get that and remove that variant
    Other(Type),
}

impl OsType {
    pub fn family(&self) -> &'static str {
        match self {
            OsType::Windows => "windows",
            OsType::MacOs => "macos",
            OsType::Linux(_) => "linux",
            OsType::Other(_) => "other",
        }
    }

    pub fn tarball_extension(&self) -> &'static str {
        match self {
            OsType::Windows => "zip",
            OsType::MacOs => "tgz",
            OsType::Linux(_) | OsType::Other(_) => "tar.gz",
        }
    }
}

fn serialize_display<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: fmt::Display,
    S: serde::Serializer,
{
    serializer.collect_str(value)
}

#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct SystemInfo {
    pub os_type: OsType,
    // AFAIK we need that for ubuntu distrib name for posit binaries
    codename: Option<String>,
    // AFAIK we need that for mac os version name (eg big sur etc) for CRAN urls
    #[serde(serialize_with = "serialize_display")]
    pub version: Version,
    arch: Option<String>,
}

impl SystemInfo {
    pub fn new(
        os_type: OsType,
        arch: Option<String>,
        codename: Option<String>,
        version: &str,
    ) -> Self {
        Self {
            os_type,
            arch,
            codename,
            version: Version::Custom(version.to_string()),
        }
    }

    pub fn from_os_info() -> Self {
        let info = os_info::get();
        let os_type = match info.os_type() {
            Type::Windows => OsType::Windows,
            // TODO: https://github.com/stanislav-tkach/os_info/pull/313
            // In the meantime, we do it manually for the main distribs and can add more as needed
            Type::Linux => OsType::Linux(""),
            Type::Ubuntu => OsType::Linux("ubuntu"),
            Type::Fedora => OsType::Linux("fedora"),
            Type::Arch => OsType::Linux("arch"),
            Type::Amazon => OsType::Linux("amazon"),
            Type::Debian => OsType::Linux("debian"),
            Type::Pop => OsType::Linux("pop"),
            Type::CentOS => OsType::Linux("centos"),
            Type::AlmaLinux => OsType::Linux("almalinux"),
            Type::openSUSE => OsType::Linux("opensuse"),
            Type::Redhat => OsType::Linux("redhat"),
            Type::RockyLinux => OsType::Linux("rocky"),
            Type::SUSE => OsType::Linux("suse"),
            Type::Gentoo => OsType::Linux("gentoo"),
            Type::Macos => OsType::MacOs,
            _ => OsType::Other(info.os_type()),
        };

        Self {
            os_type,
            codename: info.codename().map(|s| s.to_string()),
            arch: info.architecture().map(|s| s.to_string()),
            version: info.version().clone(),
        }
    }

    pub fn os_family(&self) -> &'static str {
        self.os_type.family()
    }

    pub fn codename(&self) -> Option<&str> {
        self.codename.as_deref()
    }

    pub fn arch(&self) -> Option<&str> {
        self.arch.as_deref()
    }

    /// Extract major version number from Version enum
    fn major_version(&self) -> Option<u64> {
        match &self.version {
            Version::Semantic(major, _, _) => Some(*major),
            Version::Custom(v) => {
                // Parse "8.10" -> 8, "9" -> 9, etc.
                v.split('.').next().and_then(|s| s.parse::<u64>().ok())
            }
            _ => None,
        }
    }

    /// Returns the distribution name to use for Posit Package Manager API
    /// Some distros need to be mapped to compatible API endpoints
    ///
    /// Distribution mapping strategy:
    /// - AlmaLinux 8 -> API: centos8 (most compatible for EL8)
    /// - AlmaLinux 9 -> API: rockylinux9 (centos9 unsupported)
    /// - CentOS 8 -> API: centos8
    /// - CentOS 9 -> API: rockylinux9 (centos9 returns error)
    /// - RockyLinux 8/9 -> API: rockylinux
    /// - RedHat 8/9 -> API: redhat (uses subscription-manager)
    /// - Oracle Linux -> API: redhat (binary compatible)
    ///
    /// Note: All EL8-compatible distros can use centos8 API endpoint
    ///       All EL9-compatible distros should use rockylinux9 or redhat9
    pub fn api_distribution(&self) -> &'static str {
        match self.os_type {
            OsType::Linux(distrib) => match distrib {
                "almalinux" => {
                    // AlmaLinux 8 -> centos, AlmaLinux 9 -> rockylinux
                    if let Some(major) = self.major_version() {
                        if major < 9 { "centos" } else { "rockylinux" }
                    } else {
                        log::warn!(
                            "Failed to parse major version for AlmaLinux (version: {}); sysdeps may not work correctly",
                            self.version
                        );
                        distrib
                    }
                }
                // CentOS 9 is unsupported, map to rockylinux
                "centos" => {
                    if let Some(major) = self.major_version() {
                        if major >= 9 { "rockylinux" } else { "centos" }
                    } else {
                        log::warn!(
                            "Failed to parse major version for CentOS (version: {}); sysdeps may not work correctly",
                            self.version
                        );
                        distrib
                    }
                }
                // For Oracle Linux, use redhat
                "oracle" => "redhat",
                // Everything else maps to itself
                _ => distrib,
            },
            _ => "invalid",
        }
    }

    /// Returns (distrib name, version)
    pub fn sysreq_data(&self) -> (&'static str, String) {
        match self.os_type {
            OsType::Linux(distrib) => {
                let api_distrib = self.api_distribution();
                match distrib {
                    "suse" => ("sle", self.version.to_string()),
                    "ubuntu" => {
                        let version = match self.version {
                            Version::Semantic(year, month, _) => {
                                format!("{year}.{}{month}", if month < 10 { "0" } else { "" })
                            }
                            _ => unreachable!(),
                        };
                        (api_distrib, version)
                    }
                    "debian" => match self.version {
                        Version::Semantic(major, _, _) => (api_distrib, major.to_string()),
                        _ => unreachable!(),
                    },
                    // RPM-based distributions (CentOS, AlmaLinux, RHEL, Rocky) use major version only
                    "centos" | "almalinux" | "redhat" | "rocky" | "fedora" => {
                        let version = self
                            .major_version()
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| self.version.to_string());
                        (api_distrib, version)
                    }
                    _ => (api_distrib, self.version.to_string()),
                }
            }
            _ => ("invalid", String::new()),
        }
    }
}
