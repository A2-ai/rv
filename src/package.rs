use std::collections::HashMap;
use std::fmt;
use std::fmt::Formatter;
use std::str::FromStr;

use crate::version::{Version, VersionRequirement};
use serde::{Deserialize, Serialize};

// List obtained from the REPL: `rownames(installed.packages(priority="base"))`
const BASE_PACKAGES: [&str; 14] = [
    "base",
    "compiler",
    "datasets",
    "grDevices",
    "graphics",
    "grid",
    "methods",
    "parallel",
    "splines",
    "stats",
    "stats4",
    "tcltk",
    "tools",
    "utils",
];

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, Serialize, Deserialize)]
pub enum PackageType {
    Source,
    Binary,
}

impl fmt::Display for PackageType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source => write!(f, "source"),
            Self::Binary => write!(f, "binary"),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub(crate) enum Dependency {
    Simple(String),
    Pinned {
        name: String,
        requirement: VersionRequirement,
    },
}

impl Dependency {
    pub(crate) fn name(&self) -> &str {
        match self {
            Dependency::Simple(s) => s,
            Dependency::Pinned { name, .. } => name,
        }
    }

    pub(crate) fn version_requirement(&self) -> Option<&VersionRequirement> {
        match self {
            Dependency::Simple(_) => None,
            Dependency::Pinned {
                ref requirement, ..
            } => Some(requirement),
        }
    }
}

#[derive(Debug, PartialEq, Copy, Clone, Serialize, Deserialize)]
enum OsType {
    Windows,
    Unix,
}

#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize)]
pub struct Package {
    pub(crate) name: String,
    pub(crate) version: Version,
    r_requirement: Option<VersionRequirement>,
    depends: Vec<Dependency>,
    imports: Vec<Dependency>,
    suggests: Vec<Dependency>,
    enhances: Vec<Dependency>,
    linking_to: Vec<Dependency>,
    license: String,
    md5_sum: String,
    pub(crate) path: Option<String>,
    os_type: Option<OsType>,
    recommended: bool,
    pub(crate) needs_compilation: bool,
}

#[derive(Debug, Default, PartialEq, Clone, Serialize)]
pub struct InstallationDependencies<'a> {
    pub(crate) direct: Vec<&'a Dependency>,
    pub(crate) suggests: Vec<&'a Dependency>,
}

impl Package {
    #[inline]
    pub fn works_with_r_version(&self, r_version: &Version) -> bool {
        if let Some(r_req) = &self.r_requirement {
            r_req.is_satisfied(r_version)
        } else {
            true
        }
    }

    pub fn r_version_requirement(&self) -> Option<&VersionRequirement> {
        self.r_requirement.as_ref()
    }

    pub fn dependencies_to_install(&self, install_suggestions: bool) -> InstallationDependencies {
        let mut out = Vec::with_capacity(30);
        // TODO: consider if this should be an option or just take it as an empty vector otherwise
        out.extend(self.depends.iter());
        out.extend(self.imports.iter());
        out.extend(self.linking_to.iter());

        let suggests = if install_suggestions {
            self.suggests
                .iter()
                .filter(|p| !BASE_PACKAGES.contains(&p.name()))
                .collect()
        } else {
            Vec::new()
        };

        InstallationDependencies {
            direct: out
                .into_iter()
                .filter(|p| !BASE_PACKAGES.contains(&p.name()))
                .collect(),
            suggests,
        }
    }
}

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

/// A DESCRIPTION file is like a PACKAGE file, only that it contains info about a single package
pub fn parse_description_file(content: &str) -> Option<Package> {
    // TODO: handle remotes in package for deps
    let new_content = content
        .replace("\r\n", "\n")
        .replace("\n    ", " ")
        .replace("\n  ", " ")
        .replace("  ", " ");

    let packages = parse_package_file(new_content.as_str());
    packages
        .into_values()
        .next()
        .and_then(|p| p.into_iter().next())
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

    #[test]
    fn can_parse_description_file() {
        let description = r#"
Package: scicalc
Title: Scientific Calculations for Quantitative Clinical Pharmacology and Pharmacometrics Analysis
Version: 0.1.1
Authors@R: c(
    person("Matthew", "Smith", , "matthews@a2-ai.com", role = c("aut", "cre")),
    person("Jenna", "Johnson", , "jenna@a2-ai.com", role = "aut"),
    person("Devin", "Pastoor", , "devin@a2-ai.com", role = "aut"),
    person("Wesley", "Cummings", , "wes@a2-ai.com", role = "ctb"),
    person("Emily", "Schapiro", , "emily@a2-ai.com", role = "ctb"),
    person("Ryan", "Crass", , "ryan@a2-ai.com", role = "ctb"),
    person("Jonah", "Lyon", , "jonah@a2-ai.com", role = "ctb"),
    person("Elizabeth", "LeBeau", ,"elizabeth@a2-ai.com", role = "ctb")
  )
Description: Utility functions helpful for reproducible scientific calculations.
License: MIT + file LICENSE
Encoding: UTF-8
Roxygen: list(markdown = TRUE)
RoxygenNote: 7.3.2
Imports:
    arrow,
    checkmate,
    digest,
    dplyr,
    fs,
    haven,
    magrittr,
    readr,
    readxl,
    rlang,
    stats,
    stringr
Suggests:
    knitr,
    rmarkdown,
    testthat (>= 3.0.0),
    ggplot2,
    here,
    purrr,
    pzfx,
    tools,
    tidyr
Config/testthat/edition: 3
VignetteBuilder: knitr
URL: https://a2-ai.github.io/scicalc
        "#;
        let package = parse_description_file(&description).unwrap();
        assert_eq!(package.name, "scicalc");
        assert_eq!(package.version.original, "0.1.1");
        assert_eq!(package.imports.len(), 12);
        assert_eq!(package.suggests.len(), 9);
    }
}
