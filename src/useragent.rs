use crate::{SystemInfo, Version};

pub fn build_user_agent(r_version: &Version, system_info: &SystemInfo) -> Option<String>{
    if system_info.os_family() == "linux" {
        system_info
            .arch()
            .map(|arch| format!("R/{} R ({} {}-pc-linux-gnu {} linux-gnu)", 
                r_version.original, 
                r_version.original, 
                arch, 
                arch)
            )
    } else {
        None
    }
}