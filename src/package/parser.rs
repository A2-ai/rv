//! Parses the PACKAGES files

use crate::package::remotes::parse_remote;
use crate::package::{Dependency, Package};
use crate::{Version, VersionRequirement};
use std::collections::HashMap;
use std::str::FromStr;

fn parse_dependencies(content: &str) -> Vec<Dependency> {
    let mut res = Vec::new();

    for dep in content.split(",") {
        // there are cases where dep array is constructed with a trailing comma that would give
        // an empty string
        // for example, one Depends fielf for the binr in the posit db looked like:
        // Depends: R (>= 2.15),
        if dep.is_empty() {
            continue;
        }
        let dep = dep.trim();
        if let Some(start_req) = dep.find('(') {
            let name = dep[..start_req].trim();
            let req = dep[start_req..].trim();
            let requirement = VersionRequirement::from_str(req).expect("TODO");
            res.push(Dependency::Pinned {
                name: name.to_string(),
                requirement,
            });
        } else {
            res.push(Dependency::Simple(dep.to_string()));
        }
    }

    res
}

/// Parse a PACKAGE file into something usable to resolve dependencies.
/// A package may be present multiple times in the file. If that's the case
/// we do the following:
/// 1. Filter packages by R version
/// 2. Get the first that match in the vector (the vector is in reversed order of appearance in PACKAGE file)
///
/// This assumes the content is valid and does not contain errors. It will panic otherwise.
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
                "Depends" => {
                    for p in parse_dependencies(parts[1]) {
                        if p.name() == "R" {
                            package.r_requirement = p.version_requirement().cloned();
                        } else {
                            package.depends.push(p);
                        }
                    }
                }
                "Imports" => package.imports = parse_dependencies(parts[1]),
                "LinkingTo" => package.linking_to = parse_dependencies(parts[1]),
                "Suggests" => package.suggests = parse_dependencies(parts[1]),
                "Enhances" => package.enhances = parse_dependencies(parts[1]),
                "License" => package.license = parts[1].to_string(),
                "MD5sum" => package.md5_sum = parts[1].to_string(),
                "NeedsCompilation" => package.needs_compilation = parts[1] == "yes",
                "Path" => package.path = Some(parts[1].to_string()),
                "Priority" => {
                    if parts[1] == "recommended" {
                        package.recommended = true;
                    }
                }
                "Remotes" => {
                    let remotes = parts[1]
                        .trim()
                        .split(",")
                        .map(|x| (x.to_string(), parse_remote(x.trim())))
                        .collect::<Vec<_>>();
                    for (original, out) in remotes {
                        package.remotes.insert(original, out);
                    }
                }
                // Posit uses that, maybe we can parse it?
                "SystemRequirements" => continue,
                _ => continue,
            }
        }

        // We might have some spurious empty packages depending on lines, skip those
        if name.is_empty() {
            continue;
        }

        package.name = name.clone();
        if let Some(p) = packages.get_mut(&name) {
            p.push(package);
        } else {
            packages.insert(name, vec![package]);
        }
    }

    packages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_dependencies() {
        let res = parse_dependencies("stringr, testthat (>= 1.0.2), httr(>= 1.1.0), yaml");

        assert_eq!(
            res,
            vec![
                Dependency::Simple("stringr".to_string()),
                Dependency::Pinned {
                    name: "testthat".to_string(),
                    requirement: VersionRequirement::from_str("(>= 1.0.2)").unwrap()
                },
                Dependency::Pinned {
                    name: "httr".to_string(),
                    requirement: VersionRequirement::from_str("(>= 1.1.0)").unwrap()
                },
                Dependency::Simple("yaml".to_string()),
            ]
        );
    }

    #[test]
    fn can_parse_dependencies_with_trailing_comma() {
        // This is a real case from the CRAN db that caused an early bug where an additional empty simple
        // dependency was created
        let res = parse_dependencies("R (>= 2.1.5),");

        assert_eq!(
            res,
            vec![Dependency::Pinned {
                name: "R".to_string(),
                requirement: VersionRequirement::from_str("(>= 2.1.5)").unwrap()
            },]
        );
    }

    // PACKAGE file taken from https://packagemanager.posit.co/cran/2024-12-16/src/contrib/PACKAGES
    #[test]
    fn can_parse_cran_like_package_file() {
        let content = std::fs::read_to_string("src/tests/package_files/posit-src.PACKAGE").unwrap();

        let packages = parse_package_file(&content);
        assert_eq!(packages.len(), 21811);
        let cluster_packages = &packages["cluster"];
        assert_eq!(cluster_packages.len(), 2);
        // Order from the file is kept
        assert_eq!(cluster_packages[0].version.to_string(), "2.1.7");
        assert_eq!(cluster_packages[1].version.to_string(), "2.1.8");
        assert_eq!(
            cluster_packages[1]
                .r_requirement
                .clone()
                .unwrap()
                .to_string(),
            "(>= 3.5.0)"
        );
        assert_eq!(packages["zyp"].len(), 2);
    }

    // PACKAGE file taken from https://cran.r-project.org/bin/macosx/big-sur-arm64/contrib/4.4/PACKAGES
    // Same format with fewer fields
    #[test]
    fn can_parse_cran_binary_package_file() {
        let content =
            std::fs::read_to_string("src/tests/package_files/cran-binary.PACKAGE").unwrap();
        let packages = parse_package_file(&content);
        assert_eq!(packages.len(), 22361);
    }
}
