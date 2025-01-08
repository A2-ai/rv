#[doc(inline)]

use crate::{OsType, SystemInfo};
use url::Url;

#[derive(Debug)]
/// CRAN-type repositories behave under a set of rules, but some known repositories have different nuanced behavior
pub enum RepoServer<'a> {
    /// Posit Package Manager (PPM) has linux binaries for various distributions and has immutable snapshots
    /// 
    /// Base URL: <https://packagemanager.posit.co/cran>
    PositPackageManager(&'a str), 
    /// The Metrum Package Network (MPN) has immutable snapshots and more limited in the number of packages.
    /// Only supports binaries for R/4.2 and 4.3
    /// 
    /// Base URL: <https://mpn.metworx.com>
    MetrumPackageNetwork(&'a str),
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
        if url.contains("packagemanager.posit.co/cran") {
            Self::PositPackageManager(url)
        } else if url.contains("mpn.metworx.com/snapshots/stable") {
            Self::MetrumPackageNetwork(url)
        } else if url.contains("TODO: rv url to match on") {
            Self::RV(url)
        } else {
            Self::Other(url)
        }
    }

    fn to_url(&self) -> &str {
        match self {
            Self::MetrumPackageNetwork(url)
            | Self::PositPackageManager(url)
            | Self::RV(url)
            | Self::Other(url) => url,
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
    ///     * MPN does not follow the convention for R/4.3, and hosts their binaries under `/bin/macosx/contrib/4.3`
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
    ) -> String {
        match sysinfo.os_type {
            OsType::Windows => self.get_windows_url(file_name, r_version),
            OsType::MacOs => self.get_mac_url(file_name, r_version, sysinfo),
            OsType::Linux(_) => self.get_linux_url(file_name, r_version, sysinfo),
            OsType::Other(_) => self.get_source_path(file_name),
        }
    }

    pub fn get_source_path(&self, file_name: &str) -> String {
        let url = self.to_url();
        format!("{url}/src/contrib/{file_name}")
    }

    fn get_windows_url(&self, file_name: &str, r_version: &[u32; 2]) -> String {
        // if its a Metrum URL and R version is not 4.2 or 4.3
        if let Self::MetrumPackageNetwork(_) = self {
            if r_version != &[4u32, 2u32] && r_version != &[4u32, 3u32] {
                todo!("Metrum only supports binaries for R/4.2 and 4.3")
            }
        }
        format!(
            "{}/bin/windows/contrib/{}.{}/{file_name}",
            self.to_url(),
            r_version[0],
            r_version[1]
        )
    }

    fn get_mac_url(&self, file_name: &str, r_version: &[u32; 2], sysinfo: &SystemInfo) -> String {
        if r_version[0] < 4 {
            todo!("Not supported on most repos")
        }
        let path_segments = if r_version[1] <= 2 {
            format!("bin/macosx/contrib/{}.{}", r_version[0], r_version[1])
        } else if let Self::MetrumPackageNetwork(_) = self {
            if r_version[1] == 3 {
                format!("bin/macosx/contrib/{}.{}", r_version[0], r_version[1])
            } else {
                todo!("MPN does not support > 4.2 mac binaries");
            }
        } else {
            if let Some(arch) = sysinfo.arch() {
                format!(
                    "bin/macosx/big-sur-{arch}/contrib/{}.{}",
                    r_version[0], r_version[1]
                )
            } else {
                todo!("arch not found!")
            }
        };

        format!("{}/{path_segments}/{file_name}", self.to_url())
    }

    fn get_linux_url(&self, file_name: &str, r_version: &[u32; 2], sysinfo: &SystemInfo) -> String {
        // split based on known repositories with linux binaries
        match self {
            Self::PositPackageManager(url) | Self::RV(url) => {
                Self::get_linux_binary_url(url, file_name, r_version, sysinfo)
            }
            _ => self.get_source_path(file_name),
        }
    }

    fn get_linux_binary_url(
        url: &str,
        file_name: &str,
        r_version: &[u32; 2],
        sysinfo: &SystemInfo,
    ) -> String {
        let mut url = Url::parse(url).unwrap();

        //Insert __linux__/<distribution>
        let mut segments: Vec<_> = url
            .path_segments()
            .unwrap()
            .filter(|s| s.len() != 0)
            .collect();
        if segments.is_empty() {
            return url.to_string();
        };
        let snapshot = segments.pop().unwrap();
        url.set_path(
            format!(
                "{}/__linux__/{}/{snapshot}/src/contrib/{file_name}",
                segments.join("/"),
                sysinfo.codename().unwrap()
            )
            .as_str(),
        );

        //Insert query
        url.query_pairs_mut()
            .append_pair("r_version", &format!("{}.{}", r_version[0], r_version[1]));
        if let Some(arch) = sysinfo.arch() {
            url.query_pairs_mut().append_pair("arch", arch);
        }

        url.to_string()
    }
}

mod tests {
    use super::*;
    fn ppm_url() -> String {
        "https://packagemanager.posit.co/cran/latest".to_string()
    }
    fn mpn_url() -> String {
        "https://mpn.metworx.com/snapshots/stable/2024-11-20".to_string()
    }

    #[test]
    fn test_source_url() {
        let source_url = RepoServer::from_url(&ppm_url()).get_source_path("test-file");
        let ref_url = format!("{}/src/contrib/test-file", ppm_url());
        assert_eq!(source_url, ref_url);
    }
    #[test]
    fn test_windows_url() {
        let sysinfo = SystemInfo::new(OsType::Windows, Some("x86_64".to_string()), None, "");
        let source_url =
            RepoServer::from_url(&ppm_url()).get_binary_path("test-file", &[4, 4], &sysinfo);
        let ref_url = format!("{}/bin/windows/contrib/4.4/test-file", ppm_url());
        assert_eq!(source_url, ref_url)
    }

    #[test]
    #[should_panic]
    fn test_windows_44_mpn_url() {
        let sysinfo = SystemInfo::new(OsType::Windows, Some("x86_64".to_string()), None, "");
        let _ =
            RepoServer::from_url(&mpn_url()).get_binary_path("test-file", &[4, 4], &sysinfo);
    }

    #[test]
    fn test_windows_42_mpn_url() {
        let sysinfo = SystemInfo::new(OsType::Windows, Some("x86_64".to_string()), None, "");
        let source_url =
            RepoServer::from_url(&mpn_url()).get_binary_path("test-file", &[4, 2], &sysinfo);
        let ref_url = format!("{}/bin/windows/contrib/4.2/test-file", mpn_url());
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_42_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url =
            RepoServer::from_url(&ppm_url()).get_binary_path("test-file", &[4, 2], &sysinfo);
        let ref_url = format!("{}/bin/macosx/contrib/4.2/test-file", ppm_url());
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arch64".to_string()), None, "");
        let source_url =
            RepoServer::from_url(&ppm_url()).get_binary_path("test-file", &[4, 4], &sysinfo);
        let ref_url = format!(
            "{}/bin/macosx/big-sur-arch64/contrib/4.4/test-file",
            ppm_url()
        );
        assert_eq!(source_url, ref_url)
    }

    #[test]
    #[should_panic]
    fn test_mac_mpn_44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let _ =
            RepoServer::from_url(&mpn_url()).get_binary_path("test-file", &[4, 4], &sysinfo);
    }

    #[test]
    fn test_mac_mpn_43_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url =
            RepoServer::from_url(&mpn_url()).get_binary_path("test-file", &[4, 3], &sysinfo);
        let ref_url = format!("{}/bin/macosx/contrib/4.3/test-file", mpn_url());
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
        let source_url =
            RepoServer::from_url(&ppm_url()).get_binary_path("test-file", &[4, 2], &sysinfo);
        let ref_url = "https://packagemanager.posit.co/cran/__linux__/jammy/latest/src/contrib/test-file?r_version=4.2&arch=x86_64".to_string();
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_linux_url() {
        let sysinfo = SystemInfo::new(
            OsType::Linux("ubuntu"),
            Some("x86_64".to_string()),
            Some("jammy".to_string()),
            "22.04",
        );
        let source_url = RepoServer::from_url("https://cran.rstudio.com").get_binary_path(
            "test-file",
            &[4, 4],
            &sysinfo,
        );
        let ref_url = "https://cran.rstudio.com/src/contrib/test-file".to_string();
        assert_eq!(source_url, ref_url)
    }
}
