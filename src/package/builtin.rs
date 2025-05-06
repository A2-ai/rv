use bincode::{Decode, Encode};
use fs_err as fs;
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use crate::RCmd;
use crate::package::{Package, parse_description_file_in_folder};

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct BuiltinPackages {
    pub(crate) packages: HashMap<String, Package>,
}

impl BuiltinPackages {
    /// If we fail to read it, consider we don't have it, no need to error
    pub fn load(path: impl AsRef<Path>) -> Option<Self> {
        let reader = BufReader::new(std::fs::File::open(path.as_ref()).ok()?);

        bincode::decode_from_reader(reader, bincode::config::standard()).ok()
    }

    pub fn persist(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = BufWriter::new(std::fs::File::create(path.as_ref())?);
        bincode::encode_into_std_write(self, &mut writer, bincode::config::standard())
            .expect("valid data");

        Ok(())
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
                        builtins.packages.insert(p.name.clone(), p);
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
