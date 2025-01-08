//! https://cran.r-project.org/doc/manuals/R-admin.html#Setting-up-a-package-repository-1

use crate::{OsType, SystemInfo, Version};
use url::Url;

#[derive(Debug)]
pub enum RepoClass {
    PPM(String),
    MPN(String),
    RV(String),
    Other(String),
}

impl RepoClass {
    pub fn from_url(url: &str) -> Self {
        let url = url.to_string();
        if url.contains("packagemanager.posit.co/cran") {
            RepoClass::PPM(url)
        } else if url.contains("mpn.metworx.com/snapshots/stable") {
            RepoClass::MPN(url)
        } else if url.contains("TODO: rv url to match on") {
            RepoClass::RV(url)
        } else {
            RepoClass::Other(url)
        }
    }

    fn to_url(&self) -> &String {
        match self {
            RepoClass::MPN(url) | 
            RepoClass::PPM(url) | 
            RepoClass::RV(url) |
            RepoClass::Other(url) => url
        }
    }

    pub fn get_repo_path(&self, 
        file_name: &str, 
        r_version: &[u32; 2], 
        sysinfo: &SystemInfo,
        source: bool) -> String {
        
        if source { return self.get_source_path(file_name) }
        let [major, minor] = *r_version;
        match sysinfo.os_type {
            OsType::Windows => format!("{}/bin/windows/contrib/{major}.{minor}/{file_name}", self.to_url()),
            OsType::MacOs => self.get_mac_url(file_name, &[major, minor], sysinfo),
            OsType::Linux(_) => self.get_linux_url(file_name, &[major, minor], sysinfo),
            OsType::Other(_) => self.get_source_path(file_name),
        }
    }

    fn get_source_path(&self, file_name: &str) -> String {
        let url = self.to_url();
        format!("{url}/src/contrib/{file_name}")
    }

    fn get_mac_url(&self, file_name: &str, r_version: &[u32; 2], sysinfo: &SystemInfo) -> String {
        if r_version[0] < 4 { todo!("Not supported on most repos") }
        let ext = if r_version[1] <= 2 {
            format!("bin/macosx/contrib/{}.{}", r_version[0], r_version[1])
        } else {
            if let RepoClass::MPN(_) = self {
                todo!("MPN does not support > 4.2 mac binaries");
                /*
                $ curl -I "https://mpn.metworx.com/snapshots/stable/2024-11-20/bin/macosx/big-sur-x86_64/contrib/4.3/PACKAGES"
                    HTTP/2 404 
                    x-amz-error-code: NoSuchKey
                    x-amz-error-message: The specified key does not exist.
                    x-amz-error-detail-key: snapshots/stable/2024-11-20/bin/macosx/big-sur-x86_64/contrib/4.3/PACKAGES
                */
            }
            if let Some(arch) = sysinfo.arch() {
                format!("bin/macosx/big-sur-{arch}/contrib/{}.{}", r_version[0], r_version[1])
            } else {
                todo!("arch not found!")
            }
        };

        format!("{}/{ext}/{file_name}", self.to_url())
    }

    fn get_linux_url(&self, file_name: &str, r_version: &[u32; 2], sysinfo: &SystemInfo) -> String {
        match self {
            Self::PPM(url) | Self::RV(url) => get_linux_binary_url(url, file_name, r_version, sysinfo),
            _ => self.get_source_path(file_name),
        }
    }
}

fn get_linux_binary_url(url: &str, file_name: &str, r_version: &[u32; 2], sysinfo: &SystemInfo) -> String {
    let mut url = Url::parse(url).unwrap();

    //Insert __linux__/<distribution>
    let mut segments: Vec<_> = url.path_segments().unwrap().filter(|s| s.len() != 0).collect();
    if segments.is_empty() { return url.to_string() };
    let snapshot = segments.pop().unwrap();
    url
        .set_path(format!("{}/__linux__/{}/{snapshot}/src/contrib/{file_name}", segments.join("/"), sysinfo.codename().unwrap()).as_str());

    //Insert query
    let query = sysinfo.arch().map(|arch| format!("r_version={}.{}&arch={arch}", r_version[0], r_version[1]));
    url.set_query(query.as_deref());

    url.to_string()
}

mod tests {
    use crate::system_info;

    use super::*;
    fn ppm_url() -> String {"https://packagemanager.posit.co/cran/latest".to_string()}
    fn mpn_url() -> String {"https://mpn.metworx.com/snapshots/stable/2024-11-20".to_string()}

    #[test]
    fn test_source_url() {
        let sysinfo = SystemInfo::new(OsType::Linux("ubuntu"), Some("x86_64".to_string()), Some("jammy".to_string()), "24H2");
        let source_url = RepoClass::from_url(&ppm_url())
            .get_repo_path("test-file", &[4, 4], &sysinfo, true);
        let ref_url = format!("{}/src/contrib/test-file", ppm_url());
        assert_eq!(source_url, ref_url);
    }
    #[test]
    fn test_windows_url() {
        let sysinfo = SystemInfo::new(OsType::Windows, Some("x86_64".to_string()), None, "");
        let source_url = RepoClass::from_url(&ppm_url())
            .get_repo_path("test-file", &[4, 4], &sysinfo, false);
        let ref_url = format!("{}/bin/windows/contrib/4.4/test-file", ppm_url());
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_42_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = RepoClass::from_url(&ppm_url())
            .get_repo_path("test-file", &[4, 2], &sysinfo, false);
        let ref_url = format!("{}/bin/macosx/contrib/4.2/test-file", ppm_url());
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_mac_44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("arch64".to_string()), None, "");
        let source_url = RepoClass::from_url(&ppm_url())
            .get_repo_path("test-file", &[4, 4], &sysinfo, false);
        let ref_url = format!("{}/bin/macosx/big-sur-arch64/contrib/4.4/test-file", ppm_url());
        assert_eq!(source_url, ref_url)
    }

    #[test]
    #[should_panic]
    fn test_mac_mpn_44_url() {
        let sysinfo = SystemInfo::new(OsType::MacOs, Some("x86_64".to_string()), None, "");
        let source_url = RepoClass::from_url(&mpn_url())
            .get_repo_path("test-file", &[4, 4], &sysinfo, false);
        println!("{}", source_url)
    }

    #[test]
    fn test_linux_binaries_url() {
        let sysinfo = SystemInfo::new(OsType::Linux("ubuntu"), Some("x86_64".to_string()), Some("jammy".to_string()), "22.04");
        let source_url = RepoClass::from_url(&ppm_url())
            .get_repo_path("test-file", &[4, 2], &sysinfo, false);
        let ref_url = "https://packagemanager.posit.co/cran/__linux__/jammy/latest/src/contrib/test-file?r_version=4.2&arch=x86_64".to_string();
        assert_eq!(source_url, ref_url)
    }

    #[test]
    fn test_linux_url() {
        let sysinfo = SystemInfo::new(OsType::Linux("ubuntu"), Some("x86_64".to_string()), Some("jammy".to_string()), "22.04");
        let source_url = RepoClass::from_url("https://cran.rstudio.com")
            .get_repo_path("test-file", &[4, 4], &sysinfo, false);
        let ref_url = "https://cran.rstudio.com/src/contrib/test-file".to_string();
        assert_eq!(source_url, ref_url)
    }
}
