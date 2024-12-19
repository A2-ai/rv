use crate::package::{parse_package_file, Package, PackageType};
use crate::version::Version;
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Debug, Default, PartialEq, Clone)]
pub struct RepositoryDatabase {
    pub(crate) name: String,
    source_packages: HashMap<String, Vec<Package>>,
    // Binary will have a single package for each package, no multiple
    // depending on the R version but we keep the Vec so the resolver code can work
    // for both binary and source
    // But each R version will get different binary package database
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

    // TODO: depending on how we pass things, we might already have a Version here
    pub fn parse_binary(&mut self, content: &str, r_version: &str) {
        let r_version = Version::from_str(r_version).expect("TODO").major_minor();
        let packages = parse_package_file(content);
        self.binary_packages.insert(r_version, packages);
    }

    // We always prefer binary unless `force_source` is set to true
    pub fn find_package<'a>(
        &'a self,
        name: &str,
        r_version: &Version,
        force_source: bool,
    ) -> Option<(&'a Package, PackageType)> {
        let find_package = |packages: &'a HashMap<String, Vec<Package>>| -> Option<&'a Package> {
            // If we find that package in the database we grab the first version that matches
            // the R version.
            // The package vec is already in the right order in the database.
            packages
                .get(&name.to_lowercase())
                .and_then(|p| p.iter().find(|p2| p2.works_with_r_version(r_version)))
        };

        if !force_source {
            if let Some(packages) = self.binary_packages.get(&r_version.major_minor()) {
                if let Some(package) = find_package(packages) {
                    return Some((package, PackageType::Binary));
                }
            }
        }
        println!("{:?}", self.binary_packages);

        find_package(&self.source_packages).and_then(|p| Some((p, PackageType::Source)))
    }
}
