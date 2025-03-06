use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use fs_err as fs;
use serde::{Deserialize, Serialize};

use crate::consts::{
    DESCRIPTION_FILENAME, LIBRARY_METADATA_FILENAME, LIBRARY_ROOT_DIR_NAME, RV_DIR_NAME,
};
use crate::fs::mtime_recursive;
use crate::lockfile::Source;
use crate::package::parse_version;
use crate::{ResolvedDependency, SystemInfo, Version};

/// Builds the path for binary in the cache and the library based on system info and R version
/// {R_Version}/{arch}/{codename}/
fn get_current_system_path(system_info: &SystemInfo, r_version: [u32; 2]) -> PathBuf {
    let mut path = PathBuf::new().join(format!("{}.{}", r_version[0], r_version[1]));

    if let Some(arch) = system_info.arch() {
        path = path.join(arch);
    }
    if let Some(codename) = system_info.codename() {
        path = path.join(codename);
    }

    path
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum LocalMetadata {
    /// For local folders/tarballs. The mtime of the source folder at the time of building
    Mtime(i64),
    /// For git repositories and URL sources
    Sha(String),
}

impl LocalMetadata {
    pub fn load(folder: impl AsRef<Path>) -> Result<Option<Self>, std::io::Error> {
        let path = folder.as_ref().join(LIBRARY_METADATA_FILENAME);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(path)?;
        Ok(Some(serde_json::from_str(&content).expect("valid json")))
    }

    pub fn write(&self, folder: impl AsRef<Path>) -> Result<(), std::io::Error> {
        let path = folder.as_ref().join(LIBRARY_METADATA_FILENAME);
        let mut f = fs::File::create(&path)?;
        f.write_all(serde_json::to_string(self).unwrap().as_bytes())?;
        Ok(())
    }

    pub fn sha(&self) -> Option<&str> {
        match self {
            LocalMetadata::Mtime(_) => None,
            LocalMetadata::Sha(s) => Some(s.as_str()),
        }
    }

    pub fn mtime(&self) -> Option<i64> {
        match self {
            LocalMetadata::Mtime(i) => Some(*i),
            LocalMetadata::Sha(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    /// This is the path where the packages are installed so
    /// rv/library/{R version}/{arch}/{codename?}/
    path: PathBuf,
    pub packages: HashMap<String, Version>,
    /// We keep track of all packages not coming from a package repository
    pub non_repo_packages: HashMap<String, LocalMetadata>,
    /// The folders exist but we can't find the DESCRIPTION file.
    /// This is likely a broken symlink and we should remove that folder/reinstall it
    /// It could also be something that is not a R package added by another tool
    pub broken: HashSet<String>,
}

impl Library {
    pub fn new(
        project_dir: impl AsRef<Path>,
        system_info: &SystemInfo,
        r_version: [u32; 2],
    ) -> Library {
        let system_path = get_current_system_path(system_info, r_version);
        let path = project_dir
            .as_ref()
            .join(RV_DIR_NAME)
            .join(LIBRARY_ROOT_DIR_NAME)
            .join(system_path);

        Self {
            path,
            packages: HashMap::new(),
            non_repo_packages: HashMap::new(),
            broken: HashSet::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Finds the content of the library: packages, their version and their metadata (sha/mtime)
    /// if they are not installed via a package repository
    /// Also figures out if we can access the DESCRIPTION file, if we can't
    /// it's likely that some linking between the cache and the library broke
    /// and we should not consider them installed.
    pub fn find_content(&mut self) {
        if !self.path.is_dir() {
            return;
        }

        self.packages.clear();
        self.non_repo_packages.clear();
        self.broken.clear();

        for entry in fs::read_dir(&self.path).unwrap() {
            let entry = entry.expect("Valid entry");
            // Then try to find the DESCRIPTION file and read it for the version.
            // the package name will be the name of the folder
            let path = entry.path();
            let name = path.file_name().unwrap().to_str().unwrap();
            let desc_path = path.join(DESCRIPTION_FILENAME);
            if !desc_path.exists() {
                self.broken.insert(name.to_string());
                continue;
            }

            if let Some(metadata) = LocalMetadata::load(&path).unwrap() {
                self.non_repo_packages.insert(name.to_string(), metadata);
            }

            match parse_version(desc_path) {
                Ok(version) => {
                    self.packages.insert(name.to_string(), version);
                }
                Err(_) => {
                    self.broken.insert(name.to_string());
                }
            }
        }
    }

    pub fn contains_package(&self, pkg: &ResolvedDependency) -> bool {
        if !self.packages.contains_key(pkg.name.as_ref()) {
            return false;
        }

        match pkg.source {
            Source::Git { ref sha, .. } | Source::Url { ref sha, .. } => self
                .non_repo_packages
                .get(pkg.name.as_ref())
                .map(|m| m.sha().unwrap() == sha.as_str())
                .unwrap_or(false),
            // TODO: check if mtimes works with an archive
            Source::Local { ref path } => {
                if let Some(metadata) = self.non_repo_packages.get(pkg.name.as_ref()) {
                    let local_mtime = match mtime_recursive(path) {
                        Ok(m) => m,
                        Err(_) => return false,
                    };
                    local_mtime.unix_seconds() == metadata.mtime().unwrap()
                } else {
                    false
                }
            }
            Source::Repository { .. } => &self.packages[pkg.name.as_ref()] == pkg.version.as_ref(),
        }
    }
}
