//! For R we will need some information on what is the current OS.
//! We can get that information from the `os_info` crate but we don't want to expose its type
//! to the library/CLI.
//! Instead, we encode the data we care about in an enum that can easily be shared
use os_info::{Type, Version};
use serde::Serialize;

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

#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct SystemInfo {
    pub os_type: OsType,
    // AFAIK we need that for ubuntu distrib name for posit binaries
    codename: Option<String>,
    // AFAIK we need that for mac os version name (eg big sur etc) for CRAN urls
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
            version: Version::from_string(version),
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
            Type::openSUSE => OsType::Linux("opensuse"),
            Type::Redhat => OsType::Linux("redhat"),
            Type::RockyLinux => OsType::Linux("rocky"),
            Type::SUSE => OsType::Linux("suse"),
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
}
