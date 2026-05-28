//! Parses the PACKAGES files

use crate::package::remotes::parse_remote;
use crate::package::{Dependency, NeedsEntry, Package};
use crate::{Version, VersionRequirement};
use regex::Regex;
use std::collections::HashMap;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::LazyLock;

// [\w/]+ instead of \w+ to capture slash-separated keys like Config/Needs/website
static PACKAGE_KEY_VAL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(?P<key>[\w/]+):(?P<value>.*(?:\n\s+.*)*)").unwrap());
static ANY_SPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// Parses the comma-separated value of a `Config/Needs/*` field into a list of entries.
/// Returns `None` if the value is empty or contains only whitespace.
pub fn parse_needs_entries(value: &str) -> Vec<NeedsEntry> {
    value
        .split(',')
        .filter(|t| !t.trim().is_empty())
        .map(|token| {
            // Parses one token from a `Config/Needs/*` value into a `NeedsEntry`.
            // Tokens containing `/` or `::` are treated as remote shorthands (e.g. `tidyverse/tidytemplate`);
            // all others are plain package names, optionally with a version requirement.
            let token = token.trim();
            if token.contains('/') || token.contains("::") {
                let (name, remote) = parse_remote(token);
                let pkg_name = name.unwrap_or_else(|| token.to_string());
                NeedsEntry::Remote(pkg_name, remote)
            } else {
                let dep = parse_dependencies(token)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| Dependency::Simple(token.to_string()));
                NeedsEntry::Package(dep)
            }
        })
        .collect()
}

pub fn parse_dependencies(content: &str) -> Vec<Dependency> {
    let mut res = Vec::new();

    for dep in content.split(",") {
        // there are cases where dep array is constructed with a trailing comma that would give
        // an empty string
        // for example, one Depends field for the binr in the posit db looked like:
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

    let parse_pkg = |content: &str| -> Package {
        let mut package = Package::default();

        for captures in PACKAGE_KEY_VAL_RE.captures_iter(content) {
            let key = captures.name("key").unwrap().as_str();
            let value = captures.name("value").unwrap().as_str();
            let value = ANY_SPACE_RE.replace_all(value, " ");
            let value = value.trim();

            match key {
                "Package" => package.name = value.to_string(),
                "Version" => {
                    package.version = Version::from_str(value).unwrap();
                }
                "Depends" => {
                    for p in parse_dependencies(value) {
                        if p.name() == "R" {
                            package.r_requirement = p.version_requirement().cloned();
                        } else {
                            package.depends.push(p);
                        }
                    }
                }
                "Imports" => package.imports = parse_dependencies(value),
                "LinkingTo" => package.linking_to = parse_dependencies(value),
                "Suggests" => package.suggests = parse_dependencies(value),
                "Enhances" => package.enhances = parse_dependencies(value),
                "License" => package.license = value.to_string(),
                "MD5sum" => package.md5_sum = value.to_string(),
                "NeedsCompilation" => package.needs_compilation = value == "yes",
                "Path" => package.path = Some(value.to_string()),
                "Priority" => {
                    if value == "recommended" {
                        package.recommended = true;
                    }
                }
                "Remotes" => {
                    let remotes = value
                        .split(",")
                        .map(|x| (x.to_string(), parse_remote(x.trim())))
                        .collect::<Vec<_>>();
                    for (original, out) in remotes {
                        package.remotes.insert(original, out);
                    }
                }
                "Built" => package.built = Some(value.to_string()),
                // Posit uses that, maybe we can parse it?
                "SystemRequirements" => continue,
                key if key.starts_with("Config/Needs/") => {
                    let need_key = key["Config/Needs/".len()..].to_string();
                    let entries = parse_needs_entries(value);
                    if !entries.is_empty() {
                        package.needs.insert(need_key, entries);
                    }
                }
                _ => continue,
            }
        }

        enrich_needs(&mut package);

        package
    };

    // packages are split by an empty line
    for pkg_data in content.replace("\r\n", "\n").split("\n\n") {
        let pkg = parse_pkg(pkg_data);
        if !pkg.name.is_empty() {
            if let Some(p) = packages.get_mut(&pkg.name) {
                p.push(pkg);
            } else {
                packages.insert(pkg.name.clone(), vec![pkg]);
            }
        }
    }

    packages
}

/// Enrich needs entries: if a package appears in Config/Needs/* without a version
/// requirement but has one in Suggests, promote it to Pinned using that requirement.
fn enrich_needs(pkg: &mut Package) {
    if pkg.needs.is_empty() {
        return;
    }

    let suggests_versions: HashMap<&str, &VersionRequirement> = pkg
        .suggests
        .iter()
        .filter_map(|dep| match dep {
            Dependency::Pinned { name, requirement } => Some((name.as_str(), requirement)),
            _ => None,
        })
        .collect();

    for entries in pkg.needs.values_mut() {
        for entry in entries.iter_mut() {
            if let NeedsEntry::Package(Dependency::Simple(name)) = &*entry {
                if let Some(&req) = suggests_versions.get(name.as_str()) {
                    *entry = NeedsEntry::Package(Dependency::Pinned {
                        name: name.clone(),
                        requirement: req.clone(),
                    });
                }
            }
        }
    }
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
        assert_eq!(packages.len(), 22362);
    }

    #[test]
    fn works_on_weird_linebreaks() {
        let content = r#"
Package: admiraldev
Version: 1.2.0
Depends: R (>= 4.1)
Imports: cli (>= 3.0.0), dplyr (>= 1.0.5), glue (>=
     1.6.0), lifecycle (>= 0.1.0), lubridate (>=
     1.7.4), purrr (>= 0.3.3), rlang (>= 0.4.4),
     stringr (>= 1.4.0), tidyr (>= 1.0.2),
     tidyselect (>= 1.0.0)
Suggests: diffdf, DT, htmltools, knitr, methods,
     pkgdown, rmarkdown, spelling, testthat (>=
     3.2.0), withr
License: Apache License (>= 2)
MD5sum: 4499ab1d94ad9e3f54d86dc12e704e3f
NeedsCompilation: no
    "#;
        let packages = parse_package_file(content);
        assert_eq!(packages.len(), 1);
    }

    #[test]
    fn works_on_gsm() {
        let mut content =
            std::fs::read_to_string("src/tests/descriptions/gsm.DESCRIPTION").unwrap();
        content += "\n";
        let packages = parse_package_file(&content);
        assert_eq!(packages.len(), 1);
    }

    #[test]
    fn works_on_shinytest2() {
        let mut content =
            std::fs::read_to_string("src/tests/descriptions/shinytest2.DESCRIPTION").unwrap();
        content += "\n";
        let packages = parse_package_file(&content);
        assert_eq!(packages.len(), 1);
        assert_eq!(
            packages["shinytest2"][0].linking_to,
            vec![Dependency::Simple("cpp11".to_string())]
        );
    }

    #[test]
    fn parses_config_needs() {
        let content = r#"
Package: ggplot2
Version: 3.5.0
Imports: scales
Suggests: testthat
Config/Needs/website: knitr, rmarkdown, tidyverse/tidytemplate
Config/Needs/coverage: covr

"#;
        let packages = parse_package_file(content);
        let pkg = &packages["ggplot2"][0];

        // Plain package names come through as Package entries
        let website = pkg.needs.get("website").expect("website needs");
        let plain_names: Vec<_> = website
            .iter()
            .filter_map(|e| match e {
                crate::package::NeedsEntry::Package(d) => Some(d.name().to_string()),
                _ => None,
            })
            .collect();
        assert!(plain_names.contains(&"knitr".to_string()));
        assert!(plain_names.contains(&"rmarkdown".to_string()));

        // Remote shorthand comes through as Remote entry
        let remote_count = website
            .iter()
            .filter(|e| matches!(e, crate::package::NeedsEntry::Remote(_, _)))
            .count();
        assert_eq!(remote_count, 1, "tidyverse/tidytemplate should be a Remote");

        // Second need key is parsed correctly
        let coverage = pkg.needs.get("coverage").expect("coverage needs");
        assert_eq!(coverage.len(), 1);
    }

    #[test]
    fn parses_config_needs_multiline() {
        let content = r#"
Package: ggplot2
Version: 3.5.0
Config/Needs/website: knitr,
    rmarkdown,
    tidyverse/tidytemplate

"#;
        let packages = parse_package_file(content);
        let pkg = &packages["ggplot2"][0];
        let website = pkg.needs.get("website").expect("website needs");

        let plain_names: Vec<_> = website
            .iter()
            .filter_map(|e| match e {
                crate::package::NeedsEntry::Package(d) => Some(d.name().to_string()),
                _ => None,
            })
            .collect();
        assert!(plain_names.contains(&"knitr".to_string()));
        assert!(plain_names.contains(&"rmarkdown".to_string()));

        let remote_count = website
            .iter()
            .filter(|e| matches!(e, crate::package::NeedsEntry::Remote(_, _)))
            .count();
        assert_eq!(remote_count, 1, "tidyverse/tidytemplate should be a Remote");
    }

    #[test]
    fn needs_entries_inherit_version_from_suggests() {
        // Packages listed in Config/Needs/* without a version requirement should pick up
        // the version constraint from Suggests when one exists there.
        let content = r#"
Package: bit64
Version: 4.8.99
Suggests:
    patrick (>= 0.3.0),
    testthat (>= 3.3.0),
    withr
Config/Needs/development: patrick, testthat

"#;
        let packages = parse_package_file(content);
        let pkg = &packages["bit64"][0];
        let dev = pkg.needs.get("development").expect("development needs");
        assert_eq!(dev.len(), 2);

        let patrick = dev.iter().find_map(|e| match e {
            NeedsEntry::Package(d) if d.name() == "patrick" => Some(d),
            _ => None,
        });
        let patrick = patrick.expect("patrick entry");
        assert_eq!(
            patrick.version_requirement().unwrap().to_string(),
            "(>= 0.3.0)",
            "patrick should inherit version requirement from Suggests"
        );

        let testthat = dev.iter().find_map(|e| match e {
            NeedsEntry::Package(d) if d.name() == "testthat" => Some(d),
            _ => None,
        });
        let testthat = testthat.expect("testthat entry");
        assert_eq!(
            testthat.version_requirement().unwrap().to_string(),
            "(>= 3.3.0)",
            "testthat should inherit version requirement from Suggests"
        );

        // withr is in Suggests without a version — should not appear in needs at all
        let withr = dev.iter().find(|e| match e {
            NeedsEntry::Package(d) => d.name() == "withr",
            _ => false,
        });
        assert!(withr.is_none(), "withr is not in Config/Needs/development");
    }

    #[test]
    fn needs_explicit_version_not_overridden_by_suggests() {
        // A version requirement already present in Config/Needs/* must not be replaced.
        let content = r#"
Package: mypkg
Version: 1.0.0
Suggests: testthat (>= 3.3.0)
Config/Needs/development: testthat (>= 2.0.0)

"#;
        let packages = parse_package_file(content);
        let pkg = &packages["mypkg"][0];
        let dev = pkg.needs.get("development").expect("development needs");
        let testthat = dev.iter().find_map(|e| match e {
            NeedsEntry::Package(d) if d.name() == "testthat" => Some(d),
            _ => None,
        });
        assert_eq!(
            testthat.unwrap().version_requirement().unwrap().to_string(),
            "(>= 2.0.0)",
            "explicit needs version should not be overridden by Suggests"
        );
    }

    #[test]
    fn parses_config_needs_version_constraints() {
        let content = r#"
Package: ggplot2
Version: 3.5.0
Config/Needs/website: knitr (>= 1.20), rmarkdown

"#;
        let packages = parse_package_file(content);
        let pkg = &packages["ggplot2"][0];
        let website = pkg.needs.get("website").expect("website needs");
        assert_eq!(website.len(), 2);

        let pinned = website.iter().find_map(|e| match e {
            crate::package::NeedsEntry::Package(d @ crate::package::Dependency::Pinned { .. }) => {
                Some(d)
            }
            _ => None,
        });
        let pinned = pinned.expect("knitr should be a pinned dependency");
        assert_eq!(pinned.name(), "knitr");
        assert_eq!(
            pinned.version_requirement().unwrap().to_string(),
            "(>= 1.20)"
        );

        let plain = website.iter().find_map(|e| match e {
            crate::package::NeedsEntry::Package(d @ crate::package::Dependency::Simple(_)) => {
                Some(d)
            }
            _ => None,
        });
        assert_eq!(
            plain.expect("rmarkdown should be Simple").name(),
            "rmarkdown"
        );
    }
}
