//! Parses the PACKAGES files

use crate::git::url::GitUrl;
use crate::package::remotes::parse_remote;
use crate::package::{Dependency, Package};
use crate::{Version, VersionRequirement};
use regex::Regex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::LazyLock;
use url::Url;

static PACKAGE_KEY_VAL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(?P<key>\w+):(?P<value>.*(?:\n\s+.*)*)").unwrap());
static ANY_SPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

fn parse_dependencies(content: &str) -> Vec<Dependency> {
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
                // Posit uses that, maybe we can parse it?
                "SystemRequirements" => continue,
                //
                "RemoteUrl" => {
                    if let Ok(url) = value.parse::<Url>() {
                        package.remote_url = Some(GitUrl::Http(url));
                    }
                }
                "RemoteSha" => package.remote_sha = Some(value.to_string()),
                "RemoteSubdir" => package.remote_subdir = Some(value.to_string()),
                _ => continue,
            }
        }

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
        let packages = parse_package_file(&content);
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
}
