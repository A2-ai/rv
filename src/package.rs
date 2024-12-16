use std::collections::HashMap;
use std::str::FromStr;

use crate::version::{PinnedVersion, Version};

#[derive(Debug, PartialEq, Clone)]
enum Dependency {
    Simple(String),
    Pinned {
        name: String,
        requirement: PinnedVersion,
    },
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct PackageDependency {
    // TODO: parse the version requirements as well
    name: String,
}

// TODO: what do we actually need
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Package {
    version: Version,
    // TODO: special case the R version?
    depends: Vec<Dependency>,
    imports: Vec<Dependency>,
    suggests: Vec<Dependency>,
    enhances: Vec<Dependency>,
    linking_to: Vec<Dependency>,
    license: String,
    md5_sum: String,
    path: Option<String>,
    needs_compilation: bool,
}

fn parse_dependencies(content: &str) -> Vec<Dependency> {
    let mut res = Vec::new();

    for dep in content.split(",") {
        let dep = dep.trim();
        if let Some(start_req) = dep.find('(') {
            let name = &dep[..start_req];
            let req = &dep[start_req..];
            let requirement = PinnedVersion::from_str(req).expect("TODO");
            res.push(Dependency::Pinned {
                name: name.trim().to_string(),
                requirement,
            });
        } else {
            res.push(Dependency::Simple(dep.to_string()));
        }
    }

    res
}

// TODO: benchmark the whole thing
pub fn parse_package_file(content: &str) -> HashMap<String, Package> {
    let mut packages: HashMap<String, Package> = HashMap::new();

    for package_data in content
        .replace("\r\n", "\n")
        .replace("\n        ", " ")
        .split("\n\n")
    {
        let mut package = Package::default();
        let mut name = String::new();
        // Then we fix the line wrapping for deps
        for line in package_data.lines() {
            let parts = line.splitn(2, ": ").collect::<Vec<&str>>();
            match parts[0] {
                "Package" => name = parts[1].to_string(),
                "Version" => {
                    package.version = Version::from_str(parts[1]).unwrap();
                }
                "Depends" => package.depends = parse_dependencies(parts[1]),
                "Imports" => package.imports = parse_dependencies(parts[1]),
                "LinkingTo" => package.linking_to = parse_dependencies(parts[1]),
                "Suggests" => package.suggests = parse_dependencies(parts[1]),
                "Enhances" => package.enhances = parse_dependencies(parts[1]),
                "License" => package.license = parts[1].to_string(),
                "MD5sum" => package.md5_sum = parts[1].to_string(),
                "NeedsCompilation" => package.needs_compilation = parts[1] == "yes",
                "Path" => package.path = Some(parts[1].to_string()),
                // TODO: check with Devin for those, especially OS_type and Archs
                "License_restricts_use" | "License_is_FOSS" | "OS_type" | "Priority" | "Archs" => {
                    continue
                }
                _ => panic!("Unexpected field: {} in PACKAGE file", parts[0]),
            }
        }

        if let Some(p) = packages.get_mut(&name) {
            // TODO: is that correct??
            if p.path.is_none() && p.version == package.version {
                p.path = package.path;
            }
        } else {
            packages.insert(name, package);
        }
    }

    packages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::Operator;

    #[test]
    fn can_parse_dependencies() {
        let res = parse_dependencies("stringr, testthat (>= 1.0.2), httr(>= 1.1.0), yaml");

        assert_eq!(
            res,
            vec![
                Dependency::Simple("stringr".to_string()),
                Dependency::Pinned {
                    name: "testthat".to_string(),
                    requirement: PinnedVersion::from_str("(>= 1.0.2)").unwrap()
                },
                Dependency::Pinned {
                    name: "httr".to_string(),
                    requirement: PinnedVersion::from_str("(>= 1.1.0)").unwrap()
                },
                Dependency::Simple("yaml".to_string()),
            ]
        );
    }

    #[test]
    fn can_parse_cran_package_file() {
        let content = std::fs::read_to_string("src/tests/PACKAGE").unwrap();

        let packages = parse_package_file(&content);
        // TODO: figure out how to represent the duplicates
        assert_eq!(packages.len(), 21806);
    }
}
