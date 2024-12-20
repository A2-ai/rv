use std::collections::HashMap;

use crate::package::{parse_package_file, Package, PackageType};
use crate::version::{Version, VersionRequirement};

#[derive(Debug, Default, PartialEq, Clone)]
pub struct RepositoryDatabase {
    pub(crate) name: String,
    source_packages: HashMap<String, Vec<Package>>,
    // Binary will have a single package for each package, no multiple
    // depending on the R version but we keep the Vec so the resolver code can work
    // for both binary and source
    // But each major.minor R version will get different binary package database
    binary_packages: HashMap<[u32; 2], HashMap<String, Vec<Package>>>,
}

impl RepositoryDatabase {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            ..Default::default()
        }
    }

    pub fn parse_source(&mut self, content: &str) {
        self.source_packages = parse_package_file(content);
    }

    pub fn parse_binary(&mut self, content: &str, r_version: &Version) {
        let packages = parse_package_file(content);
        self.binary_packages
            .insert(r_version.major_minor(), packages);
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
            db.get(&name.to_lowercase()).and_then(|packages| {
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

        if name == "zyp" {
            println!("looking for the source");
        }
        find_package(&self.source_packages).map(|p| (p, PackageType::Source))
    }
}
