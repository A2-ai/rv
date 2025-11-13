mod init;
mod migrate;
mod tree;

pub use info::format_repository_for_parsing;
pub use init::{find_r_repositories, init, init_structure};
pub use migrate::migrate_renv;
pub use tree::tree;

mod info {
    use std::ops::Deref;

    use crate::{OsType, Repository, SystemInfo, repository_urls::get_distro_name};

    pub fn format_repository_for_parsing(
        repository: &Repository,
        system_info: &SystemInfo,
    ) -> String {
        let determine_linux_url = |distro: &str| -> Option<String> {
            let mut new_url = repository.url.deref().clone();
            let path_segs = repository.url.path_segments()?.collect::<Vec<_>>();
            if path_segs.iter().any(|&s| s == "__linux__") {
                return Some(new_url.to_string());
            };

            let distro_name = get_distro_name(system_info, distro)?;
            let mut segments = repository.url.path_segments()?.collect::<Vec<_>>();
            let edition = segments.pop()?;
            segments.push("__linux__");
            segments.push(&distro_name);
            segments.push(edition);

            new_url.path_segments_mut().ok()?.clear().extend(segments);

            Some(new_url.to_string())
        };

        let new_url = if let OsType::Linux(distro) = system_info.os_type {
            determine_linux_url(distro)
        } else {
            None
        }
        .unwrap_or(repository.url().to_string());

        format!("({}, {})", repository.alias, new_url)
    }
}
