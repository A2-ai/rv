use crate::RCmd;
use crate::consts::{BASE_PACKAGES, RECOMMENDED_PACKAGES};
use crate::package::{Package, parse_description_file_in_folder};
use fs_err as fs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuiltinPackages {
    pub(crate) packages: HashMap<String, Package>,
}

impl BuiltinPackages {
    /// If we fail to read it, consider we don't have it, no need to error
    pub fn load(path: impl AsRef<Path>) -> Option<Self> {
        let bytes = std::fs::read(path.as_ref()).ok()?;
        rmp_serde::from_slice(&bytes).ok()
    }

    pub fn persist(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = rmp_serde::to_vec(self).expect("valid data");
        std::fs::write(path.as_ref(), bytes)
    }
}

pub fn get_builtin_versions_from_library(r_cmd: &impl RCmd) -> std::io::Result<BuiltinPackages> {
    match r_cmd.get_r_library() {
        Ok(p) => {
            let mut builtins = BuiltinPackages::default();
            for entry in fs::read_dir(p)? {
                let entry = entry?;
                match parse_description_file_in_folder(entry.path()) {
                    Ok(p) => {
                        if BASE_PACKAGES.contains(&p.name.as_str())
                            || RECOMMENDED_PACKAGES.contains(&p.name.as_str())
                        {
                            builtins.packages.insert(p.name.clone(), p);
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Error parsing description file in {:?}: {}",
                            entry.path(),
                            e
                        );
                        continue;
                    }
                }
            }
            Ok(builtins)
        }
        Err(e) => {
            log::error!("Failed to find library: {e}");
            Ok(BuiltinPackages::default())
        }
    }
}
