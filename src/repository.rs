use bincode::{Decode, Encode};
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use crate::package::{Package, PackageType, parse_package_file};
use crate::package::{Version, VersionRequirement};

#[derive(Debug, Default, PartialEq, Clone, Decode, Encode)]
pub struct RepositoryDatabase {
    pub(crate) url: String,
    pub(crate) source_packages: HashMap<String, Vec<Package>>,
    // Binary will have a single package for each package, no multiple
    // depending on the R version but we keep the Vec so the resolver code can work
    // for both binary and source
    // But each major.minor R version will get different binary package database
    pub(crate) binary_packages: HashMap<[u32; 2], HashMap<String, Vec<Package>>>,
}

impl RepositoryDatabase {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            ..Default::default()
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, RepositoryDatabaseError> {
        let reader = BufReader::new(
            std::fs::File::open(path.as_ref()).map_err(RepositoryDatabaseError::from_io)?,
        );

        bincode::decode_from_reader(reader, bincode::config::standard())
            .map_err(RepositoryDatabaseError::from_bincode)
    }

    pub fn persist(&self, path: impl AsRef<Path>) -> Result<(), RepositoryDatabaseError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(RepositoryDatabaseError::from_io)?;
        }
        let mut writer = BufWriter::new(
            std::fs::File::create(path.as_ref()).map_err(RepositoryDatabaseError::from_io)?,
        );
        bincode::encode_into_std_write(self, &mut writer, bincode::config::standard())
            .expect("valid data");

        Ok(())
    }

    pub fn parse_source(&mut self, content: &str) {
        self.source_packages = parse_package_file(content);
    }

    pub fn parse_binary(&mut self, content: &str, r_version: [u32; 2]) {
        let packages = parse_package_file(content);
        self.binary_packages.insert(r_version, packages);
    }

    // We always prefer binary unless `force_source` is set to true
    pub(crate) fn find_package<'a>(
        &'a self,
        name: &str,
        version_requirement: Option<&VersionRequirement>,
        r_version: &Version,
        force_source: bool,
    ) -> Option<(&'a Package, PackageType)> {
        let find_package = |db: &'a HashMap<String, Vec<Package>>| -> Option<&'a Package> {
            // If we find multiple packages matching the requirement, we grab the one with the
            // highest R requirement matching the provided R version.
            // The list of packages is in the same order as in the PACKAGE file so we start
            // from the end since latter entries have priority
            db.get(name).and_then(|packages| {
                let mut max_r_version = None;
                let mut found = None;

                for p in packages.iter().rev() {
                    if !p.works_with_r_version(r_version) {
                        continue;
                    }

                    if let Some(req) = version_requirement {
                        if !req.is_satisfied(&p.version) {
                            continue;
                        }
                    }

                    match (max_r_version, p.r_version_requirement()) {
                        (Some(_), None) => (),
                        (None, Some(v)) => {
                            max_r_version = Some(&v.version);
                            found = Some(p);
                        }
                        (Some(v1), Some(v2)) => {
                            if &v2.version > v1 {
                                max_r_version = Some(&v2.version);
                                found = Some(p);
                            }
                        }
                        (None, None) => found = Some(p),
                    }
                }

                found
            })
        };

        if !force_source {
            if let Some(db) = self.binary_packages.get(&r_version.major_minor()) {
                if let Some(package) = find_package(db) {
                    return Some((package, PackageType::Binary));
                }
            }
        }

        find_package(&self.source_packages).map(|p| (p, PackageType::Source))
    }

    pub(crate) fn get_binary_count(&self, r_version: &[u32; 2]) -> usize {
        self.binary_packages
            .get(r_version)
            .map(|db| db.len())
            .unwrap_or_default()
    }

    pub(crate) fn get_source_count(&self) -> usize {
        self.source_packages.len()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to load package database")]
#[non_exhaustive]
pub struct RepositoryDatabaseError {
    pub source: RepositoryDatabaseErrorKind,
}

impl RepositoryDatabaseError {
    fn from_io(err: std::io::Error) -> Self {
        Self {
            source: RepositoryDatabaseErrorKind::Io(err),
        }
    }

    fn from_bincode(err: bincode::error::DecodeError) -> Self {
        Self {
            source: RepositoryDatabaseErrorKind::Bincode(err),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum RepositoryDatabaseErrorKind {
    Io(#[from] std::io::Error),
    Bincode(#[from] bincode::error::DecodeError),
}
