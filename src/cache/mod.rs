pub mod disk;
mod info;
mod status;
pub mod utils;

use crate::cache::utils::get_global_cache_dir;
use crate::package::Package;
use crate::{RCmd, Source, SystemInfo, Version};
pub use disk::{DiskCache, PackagePaths};
pub use info::CacheInfo;
pub use status::{CacheStatus, InstallationStatus};
use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Cache {
    global: Option<DiskCache>,
    local: DiskCache,
}

impl Cache {
    pub fn new(
        r_version: &Version,
        system_info: SystemInfo,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let local = DiskCache::new(r_version, system_info.clone())?;
        let global = get_global_cache_dir()
            .and_then(|path| DiskCache::new_in_dir(r_version, system_info.clone(), path).ok())
            .map(|x| x.mark_readonly());
        Ok(Self { local, global })
    }

    pub fn new_in_dir(
        r_version: &Version,
        system_info: SystemInfo,
        root: impl AsRef<Path>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let local = DiskCache::new_in_dir(r_version, system_info.clone(), root)?;
        let global = get_global_cache_dir()
            .and_then(|path| DiskCache::new_in_dir(r_version, system_info.clone(), path).ok())
            .map(|x| x.mark_readonly());
        Ok(Self { local, global })
    }

    /// Finds where a package is present in the cache depending on its source.
    /// The version param is only used when the source is a repository
    pub fn get_installation_status(
        &self,
        pkg_name: &str,
        version: &str,
        source: &Source,
    ) -> CacheStatus {
        let local = self
            .local
            .get_installation_status(pkg_name, version, source);
        let global = self
            .global
            .as_ref()
            .map(|g| g.get_installation_status(pkg_name, version, source));
        CacheStatus { local, global }
    }

    pub fn system_info(&self) -> &SystemInfo {
        &self.local.system_info
    }

    pub fn r_version(&self) -> &[u32; 2] {
        &self.local.r_version
    }

    /// We don't try to get the global system requirements, it should be on the same OS (hopefully)
    pub fn get_system_requirements(&self) -> HashMap<String, Vec<String>> {
        self.local.get_system_requirements()
    }

    /// We don't need to reach the global
    pub fn get_builtin_packages_versions(
        &self,
        r_cmd: &impl RCmd,
    ) -> std::io::Result<HashMap<String, Package>> {
        let builtin = if let Some(g) = self.global.as_ref() {
            g.get_builtin_packages_versions(r_cmd)
                .ok()
                .unwrap_or_default()
        } else {
            HashMap::new()
        };

        if builtin.is_empty() {
            self.local.get_builtin_packages_versions(r_cmd)
        } else {
            Ok(builtin)
        }
    }

    pub fn local_root(&self) -> PathBuf {
        self.local.root.clone()
    }

    pub fn global_root(&self) -> Option<PathBuf> {
        self.global.as_ref().map(|x| x.root.clone())
    }

    pub fn local(&self) -> &DiskCache {
        &self.local
    }

    pub fn global(&self) -> Option<&DiskCache> {
        self.global.as_ref()
    }

    pub fn get_package_paths(
        &self,
        source: &Source,
        pkg_name: Option<&str>,
        version: Option<&str>,
    ) -> (PackagePaths, Option<PackagePaths>) {
        let local = self.local.get_package_paths(source, pkg_name, version);
        let global = self
            .global
            .as_ref()
            .map(|x| x.get_package_paths(source, pkg_name, version));
        (local, global)
    }
}
