use std::path::PathBuf;

use etcetera::BaseStrategy;
use sha2::{Digest, Sha256};

use crate::SystemInfo;

/// Builds the path for binary in the cache and the library based on system info and R version
/// {R_Version}/{arch}/{library_identifier}/
/// The library_identifier is the codename for Ubuntu/Debian or a generated identifier
/// for RHEL-family distros (e.g., centos8, rhel9)
pub fn get_current_system_path(system_info: &SystemInfo, r_version: [u32; 2]) -> PathBuf {
    let mut path = PathBuf::new().join(format!("{}.{}", r_version[0], r_version[1]));

    if let Some(arch) = system_info.arch() {
        path = path.join(arch);
    }
    if let Some(identifier) = system_info.library_identifier() {
        path = path.join(identifier);
    }

    path
}

/// Look up the env to see if a specific timeout is set, otherwise use the default value
pub fn get_packages_timeout() -> u64 {
    if let Ok(v) = std::env::var(crate::consts::PACKAGE_TIMEOUT_ENV_VAR_NAME) {
        if let Ok(v2) = v.parse() {
            v2
        } else {
            // If the variable doesn't parse into a valid number, return the default one
            crate::consts::PACKAGE_TIMEOUT
        }
    } else {
        crate::consts::PACKAGE_TIMEOUT
    }
}

/// Try to get where the rv cache dir should be
pub fn get_user_cache_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(crate::consts::CACHE_DIR_ENV_VAR_NAME) {
        return Some(PathBuf::from(p));
    }

    etcetera::base_strategy::choose_base_strategy()
        .ok()
        .map(|dirs| dirs.cache_dir().join("rv"))
}

/// Equivalent to sha256(input)[:10]
pub fn hash_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.to_ascii_lowercase().as_bytes());
    let result = format!("{:x}", hasher.finalize());
    result[..10].to_string()
}
