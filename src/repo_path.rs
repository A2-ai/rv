//! https://cran.r-project.org/doc/manuals/R-admin.html#Setting-up-a-package-repository-1

use crate::OsType;
use url::Url;

// https://packagemanager.posit.co/cran/__linux__/focal/2024-12-15

// TODO: this is only for CRAN right now. Need to add posit
pub fn get_binary_path(name: &str, r_version: &[u32; 2], os_type: &OsType, codename: Option<&str>) -> String {
    match os_type {
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
        // linux should be the only os we need for now that needs a code name
        // for linux we'll use a url join elsewhere so don't want the starting /
        // per how url::Url().join() works
        OsType::Linux(_) => format!("__linux__/{}/{}/src/contrib/", codename.unwrap(), name).to_string(),
        OsType::Other(t) => panic!("{} not supported right now", t),
    }
}

pub fn set_rversion_arch_query(url: &mut Url, r_version: &[u32; 2], arch: Option<&str>) {
    let query = arch.map(|a| format!("r_version={}.{}&arch={}", r_version[0], r_version[1], a));
    url.set_query(query.as_deref());
}
