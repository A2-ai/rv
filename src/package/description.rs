use std::fs;
use std::path::Path;

use crate::consts::DESCRIPTION_FILENAME;
use crate::package::parser::parse_package_file;
use crate::package::Package;

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

pub fn parse_description_file_in_folder(
    folder: impl AsRef<Path>,
) -> Result<Package, Box<dyn std::error::Error>> {
    let folder = folder.as_ref();
    let description_path = folder.join(DESCRIPTION_FILENAME);

    match fs::read_to_string(&description_path) {
        Ok(content) => {
            if let Some(package) = parse_description_file(&content) {
                Ok(package)
            } else {
                Err(format!("Invalid DESCRIPTION file at {}", description_path.display()).into())
            }
        }
        Err(e) => Err(format!(
            "Could not read destination file at {} {e}",
            description_path.display()
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::remotes::PackageRemote;

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
Remotes:
    insightsengineering/teal.code,
    insightsengineering/teal.data,
    insightsengineering/teal.slice
        "#;
        let package = parse_description_file(&description).unwrap();
        assert_eq!(package.name, "scicalc");
        assert_eq!(package.version.original, "0.1.1");
        assert_eq!(package.imports.len(), 12);
        assert_eq!(package.suggests.len(), 9);
        assert_eq!(package.remotes.len(), 3);
        match &package.remotes["insightsengineering/teal.code"] {
            (name, PackageRemote::Git { url, .. }) => {
                assert_eq!(url, "https://github.com/insightsengineering/teal.code");
                assert_eq!(name, &Some("teal.code".to_string()));
            }
            _ => panic!("Should have gotten a git repo"),
        }
    }
}
