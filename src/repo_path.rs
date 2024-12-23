//! https://cran.r-project.org/doc/manuals/R-admin.html#Setting-up-a-package-repository-1

use crate::{OsType, SystemInfo};

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
