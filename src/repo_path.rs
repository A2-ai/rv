use crate::{OsType, SystemInfo};
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
pub enum RepoServer<'a> {
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
    pub fn from_url(url: &'a str) -> Self {
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
    /// MacOS binaries are not widely supported for R < 4.0 and are not supported in this tooling.
    ///
    /// There is a split in the repository structure at R/4.2
    ///
    /// * For R <= 4.2, binaries are found under `/bin/macosx/contrib/4.<R version minor>`
    ///
    /// * For R > 4.2, binaries are found under `/bin/macosx/big-sur-<arch>/4.<R version minor>`
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
    pub fn get_binary_path(
        &self,
        file_name: &str,
        r_version: &[u32; 2],
        sysinfo: &SystemInfo,
    ) -> Option<String> {
        // rv does not support binaries for less than R/4.0
        if r_version[0] < 4 {
            return None;
        }

        match sysinfo.os_type {
            OsType::Windows => Some(self.get_windows_url(file_name, r_version)),
            OsType::MacOs => self.get_mac_url(file_name, r_version, sysinfo),
            OsType::Linux(distro) => self.get_linux_url(file_name, r_version, sysinfo, distro),
            OsType::Other(_) => None,
        }
    }

    pub fn get_binary_tarball_path(
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

    pub fn get_source_path(&self, file_name: &str) -> String {
        let url = self.url();
        format!("{url}/src/contrib/{file_name}")
    }

    pub fn get_source_tarball_path(&self, name: &str, version: &str, path: Option<&str>) -> String {
        let p = if let Some(p2) = path {
            format!("{p2}/")
        } else {
            String::new()
        };
        let file_name = format!("{p}{name}_{version}.tar.gz");
        self.get_source_path(&file_name)
    }

    fn get_windows_url(&self, file_name: &str, r_version: &[u32; 2]) -> String {
        format!(
            "{}/bin/windows/contrib/{}.{}/{file_name}",
            self.url(),
            r_version[0],
            r_version[1]
        )
    }

    fn get_mac_url(
        &self,
        file_name: &str,
        r_version: &[u32; 2],
        sysinfo: &SystemInfo,
    ) -> Option<String> {
        // CRAN-type repositories change the path in which Mac binaries are hosted after R/4.2
        if r_version[1] <= 2 {
            return Some(format!(
                "{}/bin/macosx/contrib/{}.{}/{file_name}",
                self.url(),
                r_version[0],
                r_version[1]
            ));
        }

        // The new Mac binary path for R >= 4.3 includes the architecture as well as the MacOS version.
        // Currently, the MacOS version on CRAN-type repositories is hardcoded to be big-sur
        if let Some(arch) = sysinfo.arch() {
            return Some(format!(
                "{}/bin/macosx/big-sur-{arch}/contrib/{}.{}/{file_name}",
                self.url(),
                r_version[0],
                r_version[1]
            ));
        }

        None
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
    fn test_windows_url() {
        let sysinfo = SystemInfo::new(OsType::Windows, Some("x86_64".to_string()), None, "");
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 4], &sysinfo)
            .unwrap();
        let ref_url = format!("{}/bin/windows/contrib/4.4/test-file", PPM_URL);
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_42_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 2], &sysinfo)
            .unwrap();
        let ref_url = format!("{}/bin/macosx/contrib/4.2/test-file", PPM_URL);
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arch64".to_string()), None, "");
        let source_url = RepoServer::from_url(PPM_URL)
            .get_binary_path("test-file", &[4, 4], &sysinfo)
            .unwrap();
        let ref_url = format!(
            "{}/bin/macosx/big-sur-arch64/contrib/4.4/test-file",
            PPM_URL
        );
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
