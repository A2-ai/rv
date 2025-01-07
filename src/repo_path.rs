//! https://cran.r-project.org/doc/manuals/R-admin.html#Setting-up-a-package-repository-1

use crate::{OsType, SystemInfo, Version};
use url::Url;

enum RepoClass {
    PPM(String),
    MPN(String),
    RV(String),
    Other(String),
}

impl RepoClass {
    fn from_url(url: &str) -> Self {
        let url = url.to_string();
        if url.contains("packagemanager.posit.co/cran") {
            RepoClass::PPM(url)
        } else if url.contains("mpn.metworx.com/snapshot/stable") {
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

    fn get_repo_path(&self, 
        file_name: &str, 
        r_version: Version, 
        sysinfo: SystemInfo,
        source: bool) -> String {
        
        if source { return self.get_source_path(file_name) }

        let [major, minor] = r_version.major_minor();
        match sysinfo.os_type {
            OsType::Windows => format!("{}/bin/windows/contrib/{major}.{minor}/{file_name}", self.to_url()),
            OsType::MacOs => self.get_mac_url(file_name, r_version, sysinfo),
            OsType::Linux(dist) => self.get_linux_url(file_name, r_version, sysinfo, dist),
            OsType::Other(_) => self.get_source_path(file_name),
        }
        /*
        match &self {
            Self::PPM(url) => {
                match sysinfo.os_type {
                    OsType::Windows => format!("{}/bin/windows/contrib/{major}.{minor}/{file_name}", &self.to_url()),
                    OsType::MacOs => self.get_mac_url(file_name, r_version, sysinfo),
                    OsType::Linux(dist) => self.get_linux_binary_url(file_name, r_version, sysinfo, dist),
                }
            },
            Self::MPN(url) => "asdf",
            Self::RV(url) => "asdf",
            Self::Other(url) => "asdf",
        };
        */
    }

    fn get_source_path(&self, file_name: &str) -> String {
        let url = self.to_url();
        format!("{url}/src/contrib/{file_name}")
    }

    fn get_mac_url(&self, file_name: &str, r_version: Version, sysinfo: SystemInfo) -> String {
        let [major, minor] = r_version.major_minor();
        let ext = if major < 4 {
            todo!("Not supported on most repos")
        } else if minor <= 2 {
            format!("/bin/macosx/contrib/4.{minor}")
        } else {
            if let Some(arch) = sysinfo.arch() {
                format!("/bin/macosx/big-sur-{arch}/4.{minor}")
            } else {
                todo!("arch not found!")
            }
        };

        format!("{}/{ext}/{file_name}", self.to_url())
    }

    fn get_linux_url(&self, file_name: &str, r_version: Version, sysinfo: SystemInfo, dist: &str) -> String {
        match self {
            Self::PPM(url) | Self::RV(url) => get_linux_binary_url(url, file_name, r_version, sysinfo, dist),
            _ => self.get_source_path(file_name),
        }
    }
}

fn get_linux_binary_url(url: &str, file_name: &str, r_version: Version, sysinfo: SystemInfo, dist: &str) -> String {
    let mut url = Url::parse(url).unwrap();

    //Insert __linux__/<distribution>
    let mut segments: Vec<_> = url.path_segments().unwrap().filter(|s| s.len() != 0).collect();
    if segments.is_empty() { return url.to_string() };
    let snapshot = segments.pop().unwrap();
    url
        .set_path(format!("{}/__linux__/{dist}/{snapshot}/src/contrib/{file_name}", segments.join("/")).as_str());

    //Insert query
    let [major, minor] = r_version.major_minor();
    let query = sysinfo.arch().map(|arch| format!("r_version={major}.{minor}&arch={arch}"));
    url.set_query(query.as_deref());

    url.to_string()
}

mod tests {
    use url::Url;

    #[test]
    fn testing() {
        let url = "https://packagemanger.posit.co/cran/latest";
        let parsed_url = Url::parse(url).unwrap();
        let mut segments = parsed_url
            .path_segments()
            .unwrap()
            .filter(|s| s.len() != 0)
            .collect::<Vec<_>>();
        println!("{:#?}", segments.last().unwrap());
        let base = segments.pop().unwrap();
        println!("{:#?}", segments);
    }
}

// https://packagemanager.posit.co/cran/__linux__/focal/2024-12-15

// TODO: this is only for CRAN right now. Need to add posit
pub fn get_binary_path(r_version: &[u32; 2], system_info: &SystemInfo) -> String {
    match system_info.os_type {
        OsType::Windows => format!("/bin/windows/contrib/{}.{}/", r_version[0], r_version[1]),
        OsType::MacOs => {
            // TODO: only cran right now
            if r_version[0] < 4 {
                todo!("TODO: not on cran")
            }
            // TODO: only arm right now (m1), need to use arch
            if r_version[0] > 2 {
                return format!(
                    "/bin/macosx/big-sur-arm64/contrib/{}.{}/",
                    r_version[0], r_version[1]
                );
            }

            todo!("Handle no binary");
        }
        OsType::Linux(_distrib) => "/src/contrib/".to_string(),
        OsType::Other(t) => panic!("{} not supported right now", t),
    }
}
/*
pub fn get_repo_path(repo_url: String, file_name: String, r_version: &[u32; 2], system_info: &SystemInfo, binary: bool) -> String {
    let [major, minor] = r_version;
    match system_info.os_type {
        OsType::Windows => format!("{repo_url}/bin/windows/contrib/{major}.{minor}/{file_name}"),
        OsType::MacOs => 
    }

}

fn mac_repo_ext(r_version: &[u32; 2], arch: Option<&str>) {
    if r_version[0] < 4 {
        todo!("TODO: not on most repositories")
    }
    if r_version[1] <= 2{
        "/bin/macosx/"
    }
}
*/
