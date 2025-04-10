use crate::consts::PACKAGE_FILENAME;
use crate::{OsType, ResolvedDependency, SystemInfo};
use regex::Regex;
use std::sync::LazyLock;

static SNAPSHOT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(latest|\d{4}-\d{2}-\d{2})$").unwrap());
static POSIT_PACKAGE_MANAGER_BASE_URL: &str = "https://packagemanager.posit.co/cran";
static RV_BASE_URL: &str = "TODO: RV base url";

/// This is based on the mapping on PPM config <https://packagemanager.posit.co/client/#/repos/cran/setup>.
fn get_distro_name(sysinfo: &SystemInfo, distro: &str) -> Option<String> {
    match distro {
        "centos" => {
            if let os_info::Version::Semantic(major, _, _) = sysinfo.version {
                if major >= 7 {
                    return Some(format!("centos{major}"));
                }
            }
            None
        }
        "rocky" => {
            // rocky linux is distributed under rhel, starting support at v9
            if let os_info::Version::Semantic(major, _, _) = sysinfo.version {
                if major >= 9 {
                    return Some(format!("rhel{major}"));
                }
            }
            None
        }
        "opensuse" | "suse" => {
            // both suse OsType's are distributed under opensuse
            if let os_info::Version::Semantic(major, minor, _) = sysinfo.version {
                if (major >= 15) && (minor >= 5) {
                    return Some(format!("opensuse{major}{minor}"));
                }
            }
            None
        }
        "redhat" => {
            // Redhat linux v7&8 are under centos. distribution changed as of v9
            if let os_info::Version::Semantic(major, _, _) = sysinfo.version {
                if major >= 9 {
                    return Some(format!("rhel{major}"));
                }
                if major >= 7 {
                    return Some(format!("centos{major}"));
                }
            }
            None
        }
        // ubuntu and debian are distributed under their codenames
        "ubuntu" | "debian" => sysinfo.codename().map(|x| x.to_string()),
        _ => None,
    }
}

/// CRAN-type repositories behave under a set of rules, but some known repositories have different nuanced behavior
/// Unless otherwise noted, each repository is assumed to have MacOS and Windows binaries for at least R > 4.0
#[derive(Debug)]
enum RepoServer<'a> {
    /// Posit Package Manager (PPM) has linux binaries for various distributions and has immutable snapshots.
    /// Of note, [PPM does NOT support binaries for Bioconductor]. Therefore we consider this variant the CRAN PPM Repo Server.
    ///
    /// [PPM does NOT support binaries for Bioconductor]: https://docs.posit.co/rspm/admin/serving-binaries/
    ///
    /// Base URL: <https://packagemanager.posit.co/cran>
    PositPackageManager(&'a str),
    /// The RV server has linux binaries for various distributions and has immutable snapshots. Other info TBD
    ///
    /// Base URL: "TBD"
    RV(&'a str),
    /// Other unrecognized repositories, including CRAN mirrors (i.e. <https://cran.r-project.org/>)
    /// The other variants are known repositories with unique behaviors. Other repositories are treated under the [base CRAN-style repository]
    ///
    /// [base CRAN-style repository]: <https://cran.r-project.org/doc/manuals/R-admin.html#Setting-up-a-package-repository-1>
    Other(&'a str),
}

impl<'a> RepoServer<'a> {
    /// Convert a url to a variant of the enum
    fn from_url(url: &'a str) -> Self {
        if url.contains(POSIT_PACKAGE_MANAGER_BASE_URL) {
            Self::PositPackageManager(url)
        } else if url.contains(RV_BASE_URL) {
            Self::RV(url)
        } else {
            Self::Other(url)
        }
    }

    fn url(&self) -> &str {
        match self {
            Self::PositPackageManager(url) | Self::RV(url) | Self::Other(url) => url,
        }
    }

    /// # Get the path to the binary version of the file provided, when available.
    ///
    /// ## Given a CRAN-type repository URL, the location of the file wanted depends on the operating system.
    /// Nuances are also encoded for the few recognized repositories in RepoServer's variants.
    ///
    /// ### Windows
    /// Windows binaries are found under `/bin/windows/contrib/<R version major>.<R version minor>`
    ///
    /// ### MacOS
    /// Binaries for arm64 processors are found under `/bin/macosx/big-sur-arm64/contrib/4.<R minor version>`
    ///
    /// Binaries for x86_64 processors are found under different paths depending on the R version
    /// * For R <= 4.2, binaries are found under `/bin/macosx/contrib/4.<R minor version>`
    ///
    /// * For R > 4.2, binaries are found under `/bin/macosx/big-sur-x86_64/contrib/4.<R minor version>`
    ///
    /// Currently, the Mac version is hard coded to Big Sur. Earlier versions are archived for earlier versions of R,
    /// but are not supported in this tooling. Later versions (sequoia) are also not yet differentiated
    ///
    /// ### Linux
    /// For most CRAN-type repositories, linux binaries do not exist. Only source packages, which are found under `/src/contrib`
    ///
    /// * Posit Package Manager and the RV server host platform/version specific linux binaries under an additional directory segments `__linux__/<distribution codename>`.
    /// * PPM and RV server are both snapshot based, so the additional directory segments are placed in from of the snapshot date (the last element) by convention.
    /// * In order to provide the correct binary for the R version and system architecture, both servers use query strings or the form `r_version=<R version major>.<R version minor>` and `arch=<system arch>`
    ///
    /// Thus the full path segment is `__linux__/<distribution codename>/<snapshot date>/src/contrib/<file name>?r_version=<R version major>.<R version minor>&arch=<system arch>`
    fn get_binary_path(
        &self,
        file_name: &str,
        r_version: &[u32; 2],
        sysinfo: &SystemInfo,
    ) -> Option<String> {
        // rv does not support binaries for less than R/3.6
        if r_version < &[3, 6] {
            return None;
        }

        match sysinfo.os_type {
            OsType::Windows => Some(self.get_windows_url(file_name, r_version)),
            OsType::MacOs => self.get_mac_url(file_name, r_version, sysinfo),
            OsType::Linux(distro) => self.get_linux_url(file_name, r_version, sysinfo, distro),
            OsType::Other(_) => None,
        }
    }

    fn get_binary_tarball_path(
        &self,
        name: &str,
        version: &str,
        path: Option<&str>,
        r_version: &[u32; 2],
        sysinfo: &SystemInfo,
    ) -> Option<String> {
        let ext = sysinfo.os_type.tarball_extension();
        let p = if let Some(p2) = path {
            format!("{p2}/")
        } else {
            String::new()
        };
        let file_name = format!("{p}{name}_{version}.{ext}");
        self.get_binary_path(&file_name, r_version, sysinfo)
    }

    fn get_source_path(&self, file_name: &str) -> String {
        let url = self.url();
        format!("{url}/src/contrib/{file_name}")
    }

    fn get_source_tarball_path(&self, name: &str, version: &str, path: Option<&str>) -> String {
        let p = if let Some(p2) = path {
            format!("{p2}/")
        } else {
            String::new()
        };
        let file_name = format!("{p}{name}_{version}.tar.gz");
        self.get_source_path(&file_name)
    }

    // Archived packages under the format <base url>/src/contrib/Archive/<pkg name>/<pkg name>_<pkg version>.tar.gz
    fn get_archive_tarball_path(&self, name: &str, version: &str) -> String {
        let file_name = format!("Archive/{name}/{name}_{version}.tar.gz");
        self.get_source_path(&file_name)
    }

    fn get_tarball_urls(
        &self,
        name: &str,
        version: &str,
        path: Option<&str>,
        r_version: &[u32; 2],
        sysinfo: &SystemInfo,
    ) -> TarballUrls {
        let source = self.get_source_tarball_path(name, version, path);
        let binary = self.get_binary_tarball_path(name, version, path, r_version, sysinfo);
        let archive = self.get_archive_tarball_path(name, version);
        TarballUrls {
            source,
            binary,
            archive,
        }
    }

    fn get_windows_url(&self, file_name: &str, r_version: &[u32; 2]) -> String {
        format!(
            "{}/bin/windows/contrib/{}.{}/{file_name}",
            self.url(),
            r_version[0],
            r_version[1]
        )
    }

    /// CRAN-type repositories have had to adapt to the introduction of the Mac arm64 processors
    /// For x86_64 processors, a split in the path to the binaries occurred at R/4.2:
    /// * R <= 4.2, the path is `/bin/macosx/contrib/4.<R minor version>`
    /// * R > 4.2, the path is `/bin/macosx/big-sur-x86_64/contrib/4.<R minor version>`
    ///
    /// This split occurred to mirror the new path pattern for arm64 processors.
    /// The path to the binaries built for arm64 binaries is `/bin/macosx/big-sur-arm64/contrib/4.<R minor version>`
    /// While CRAN itself only started supporting arm64 binaries at R/4.2, many repositories (including PPM) support binaries for older versions
    fn get_mac_url(
        &self,
        file_name: &str,
        r_version: &[u32; 2],
        sysinfo: &SystemInfo,
    ) -> Option<String> {
        // If the system architecture cannot be determined, Mac binaries are not supported
        let arch = sysinfo.arch()?;

        // If the processor is arm64, binaries will only be found on this path
        // CRAN does not officially support arm64 binaries until R/4.2, but other repositories may (i.e. PPM does)
        if arch == "arm64" {
            return Some(format!(
                "{}/bin/macosx/big-sur-{arch}/contrib/{}.{}/{file_name}",
                self.url(),
                r_version[0],
                r_version[1]
            ));
        }

        // For x86_64, the path in which binaries are found switches after R/4.2
        if r_version <= &[4, 2] {
            return Some(format!(
                "{}/bin/macosx/contrib/{}.{}/{file_name}",
                self.url(),
                r_version[0],
                r_version[1]
            ));
        }

        Some(format!(
            "{}/bin/macosx/big-sur-{arch}/contrib/{}.{}/{file_name}",
            self.url(),
            r_version[0],
            r_version[1]
        ))
    }

    fn get_linux_url(
        &self,
        file_name: &str,
        r_version: &[u32; 2],
        sysinfo: &SystemInfo,
        distro: &str,
    ) -> Option<String> {
        // PPM and RV have linux binaries, under the same format, but with different base URLs
        // All other repositories are assumed to not have linux binaries
        let dir_url = match self {
            Self::PositPackageManager(url) => {
                format!(
                    "{}/__linux__/{}/{}/src/contrib",
                    POSIT_PACKAGE_MANAGER_BASE_URL,
                    get_distro_name(sysinfo, distro)?,
                    Self::extract_snapshot_date(url)?,
                )
            }
            Self::RV(url) => format!(
                "{}/__linux__/{}/{}/src/contrib",
                RV_BASE_URL,
                get_distro_name(sysinfo, distro)?, //need to determine if RV will have same binary support/distro names
                Self::extract_snapshot_date(url)?
            ),
            Self::Other(url) => {
                //TODO: we cannot expect only snapshot date/latest pattern for other/RV/PRISM in the future
                // but this unblocks some work right now
                let snapshot_date = Self::extract_snapshot_date(url)?;
                let trimmed_url = url.trim_end_matches(snapshot_date).trim_end_matches("/");
                format!(
                    "{}/__linux__/{}/{}/src/contrib",
                    trimmed_url,
                    get_distro_name(sysinfo, distro)?, //need to determine if RV will have same binary support/distro names
                    snapshot_date
                )
            }
        };

        // binaries are only returned when query strings are set for the r version
        let mut linux_url = Some(format!(
            "{}/{}?r_version={}.{}",
            dir_url, file_name, r_version[0], r_version[1]
        ));

        //arch is not necessarily required, but appended when present
        if let Some(arch) = sysinfo.arch() {
            linux_url = Some(format!("{}&arch={arch}", linux_url?));
        };
        linux_url
    }

    fn extract_snapshot_date(url: &str) -> Option<&str> {
        SNAPSHOT_RE
            .captures(url)
            .and_then(|c| c.get(0))
            .map(|x| x.as_str())
    }
}

pub struct TarballUrls {
    pub source: String,
    pub binary: Option<String>,
    pub archive: String,
}

pub fn get_tarball_urls(
    dep: &ResolvedDependency,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> TarballUrls {
    let repo_server = RepoServer::from_url(dep.source.source_path());
    repo_server.get_tarball_urls(
        &dep.name,
        &dep.version.original,
        dep.path.as_deref(),
        r_version,
        sysinfo,
    )
}

/// Gets the source/binary url for the given filename, usually PACKAGES
/// Use `get_tarball_urls` if you want to get the package tarballs URLs
pub fn get_package_file_urls(
    url: &str,
    r_version: &[u32; 2],
    sysinfo: &SystemInfo,
) -> (String, Option<String>) {
    let repo_server = RepoServer::from_url(url);
    (
        repo_server.get_source_path(PACKAGE_FILENAME),
        repo_server.get_binary_path(PACKAGE_FILENAME, r_version, sysinfo),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    static PPM_URL: &str = "https://packagemanager.posit.co/cran/latest";

    #[test]
    fn test_source_url() {
        let source_url = RepoServer::from_url(PPM_URL).get_source_path("test-file");
        let ref_url = format!("{}/src/contrib/test-file", PPM_URL);
        assert_eq!(source_url, ref_url);
    }
    #[test]
    fn test_binary_35_url() {
        let sysinfo = SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            Some("jammy".to_string()),
            "22.04",
        );
        if let None = RepoServer::from_url(PPM_URL).get_binary_path("test-file", &[3, 5], &sysinfo)
        {
            assert!(true)
        }
    }

    #[test]
    fn test_windows_url() {
        let sysinfo = SystemInfo::new(OsType::Windows, Some("x86_64".to_string()), None, "");
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 4], &sysinfo)
            .unwrap();
        let ref_url = format!("{}/bin/windows/contrib/4.4/test-file", PPM_URL);
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_x86_64_r41_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 1], &sysinfo)
            .unwrap();
        let ref_url = format!("{}/bin/macosx/contrib/4.1/test-file", PPM_URL);
        assert_eq!(source_url, ref_url)
    }
    #[test]
    fn test_mac_arm64_r41_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arm64".to_string()), None, "");
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 1], &sysinfo)
            .unwrap();
        let ref_url = format!("{}/bin/macosx/big-sur-arm64/contrib/4.1/test-file", PPM_URL);
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_x86_64_r44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 4], &sysinfo)
            .unwrap();
        let ref_url = format!(
            "{}/bin/macosx/big-sur-x86_64/contrib/4.4/test-file",
            PPM_URL
        );
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_arm64_r44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arm64".to_string()), None, "");
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 4], &sysinfo)
            .unwrap();
        let ref_url = format!("{}/bin/macosx/big-sur-arm64/contrib/4.4/test-file", PPM_URL);
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_linux_binaries_url() {
        let sysinfo = SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            Some("jammy".to_string()),
            "22.04",
        );
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 2], &sysinfo)
            .unwrap();
        let ref_url = "https://packagemanager.posit.co/cran/__linux__/jammy/latest/src/contrib/test-file?r_version=4.2&arch=x86_64".to_string();
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_linux_cran_url() {
        let sysinfo = SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            Some("jammy".to_string()),
            "22.04",
        );
        if let None = RepoServer::from_url("https://cran.rstudio.com").get_binary_path(
            "test-file",
            &[4, 4],
            &sysinfo,
        ) {
            assert!(true)
        }
    }
}
