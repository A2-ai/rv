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
        let find_package = |packages: &'a HashMap<String, Vec<Package>>| -> Option<&'a Package> {
            // If we find that package in the database we grab the first version that matches
            // the R version and then whatever version_requirement is defined
            // The package vec is already in the right order in the database.
            packages.get(&name.to_lowercase()).and_then(|p| {
                p.iter().find(|p2| {
                    if !p2.works_with_r_version(r_version) {
                        false
                    } else {
                        if let Some(req) = version_requirement {
                            req.is_satisfied(&p2.version)
                        } else {
                            true
                        }
                    }
                })
            })
        };

        if !force_source {
            if let Some(packages) = self.binary_packages.get(&r_version.major_minor()) {
                if let Some(package) = find_package(packages) {
                    return Some((package, PackageType::Binary));
                }
            }
        }

        find_package(&self.source_packages).map(|p| (p, PackageType::Source))
    }
}
