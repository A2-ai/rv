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

#[derive(Debug, PartialEq, Copy, Clone)]
enum OsType {
    Windows,
    Unix,
}

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
    os_type: Option<OsType>,
    recommended: bool,
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
/// Parse a PACKAGE file into something usable to resolve dependencies.
/// A package may be present multiple times in the file. If that's the case
/// we do the following:
/// 1. Filter packages by R version
/// 2. Get the first that match in the vector (the vector is in reversed order of appearance in PACKAGE file)
pub fn parse_package_file(content: &str) -> HashMap<String, Vec<Package>> {
    let mut packages: HashMap<String, Vec<Package>> = HashMap::new();

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
                "OS_type" => {
                    package.os_type = Some(match parts[1] {
                        "windows" => OsType::Windows,
                        "unix" => OsType::Unix,
                        _ => panic!("Unknown OS type: {}", parts[1]),
                    });
                }
                "Priority" => {
                    if parts[1] == "recommended" {
                        package.recommended = true;
                    }
                }
                "License_restricts_use" | "License_is_FOSS" | "Archs" => continue,
                _ => panic!("Unexpected field: {} in PACKAGE file", parts[0]),
            }
        }

        if let Some(p) = packages.get_mut(&name) {
            // Insert it in front since later entries in the file have priority
            p.insert(0, package);
        } else {
            packages.insert(name, vec![package]);
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
        assert_eq!(packages.len(), 21806);
        let cluster_packages = &packages["cluster"];
        assert_eq!(cluster_packages.len(), 2);
        // The second entry is before the first one
        assert_eq!(cluster_packages[0].version.to_string(), "2.1.7");
        assert_eq!(cluster_packages[1].version.to_string(), "2.1.8");
    }
}
