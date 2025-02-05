use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::consts::{
    DESCRIPTION_FILENAME, LIBRARY_ROOT_DIR_NAME, LOCAL_MTIME_FILENAME, RV_DIR_NAME,
};
use crate::package::parse_version;
use crate::Version;
use fs_err as fs;

#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    /// This is the path where the packages are installed so
    /// rv/library/{R version}/{arch}/{codename?}/
    path: PathBuf,
    pub packages: HashMap<String, Version>,
    /// If we find a local package installed, also read the latest mtime
    pub local_packages: HashMap<String, i64>,
    /// The folders exist but we can't find the DESCRIPTION file.
    /// This is likely a broken symlink and we should remove that folder/reinstall it
    /// It could also be something that is not a R package added by another tool
    pub broken: Vec<String>,
}

impl Library {
    pub fn new(project_dir: impl AsRef<Path>, system_path: PathBuf) -> Library {
        let path = project_dir
            .as_ref()
            .join(RV_DIR_NAME)
            .join(LIBRARY_ROOT_DIR_NAME)
            .join(system_path);

        Self {
            path,
            packages: HashMap::new(),
            local_packages: HashMap::new(),
            broken: Vec::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn find_content(&mut self) {
        if !self.path.is_dir() {
            return;
        }

        self.packages.clear();
        self.local_packages.clear();
        self.broken = Vec::new();

        for entry in fs::read_dir(&self.path).unwrap() {
            let entry = entry.expect("Valid entry");
            // Then try to find the DESCRIPTION file and read it for the version.
            // the package name will be the name of the folder
            let path = entry.path();
            let name = path.file_name().unwrap().to_str().unwrap();
            let desc_path = path.join(DESCRIPTION_FILENAME);
            if !desc_path.exists() {
                self.broken.push(name.to_string());
                continue;
            }

            let mtime_path = path.join(LOCAL_MTIME_FILENAME);
            if mtime_path.exists() {
                let timestamp: i64 = fs::read_to_string(mtime_path).unwrap().parse().unwrap();
                self.local_packages.insert(name.to_string(), timestamp);
            }

            match parse_version(desc_path) {
                Ok(version) => {
                    self.packages.insert(name.to_string(), version);
                }
                Err(_) => {
                    self.broken.push(name.to_string());
                }
            }
        }
    }
}
