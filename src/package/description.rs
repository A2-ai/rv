use crate::consts::DESCRIPTION_FILENAME;
use crate::package::parser::parse_package_file;
use crate::package::Package;
use crate::Version;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::path::Path;
use std::str::FromStr;

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

/// Quick version that only cares about retrieving the version of a package and ignores everything else
pub fn parse_version(file_path: impl AsRef<Path>) -> Result<Version, Box<dyn std::error::Error>> {
    let file = File::open(file_path)?;
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        if let Some(stripped) = line.strip_prefix("Version:") {
            return Ok(Version::from_str(stripped.trim()).expect("Version should be parsable"));
        }
    }

    Err("Version not found.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::remotes::PackageRemote;

    #[test]
    fn can_parse_description_file() {
        let content = fs::read_to_string("src/tests/descriptions/gsm.app.DESCRIPTION").unwrap();
        let package = parse_description_file(&content).unwrap();
        assert_eq!(package.name, "gsm.app");
        assert_eq!(package.version.original, "2.3.0.9000");
        assert_eq!(package.imports.len(), 15);
        assert_eq!(package.suggests.len(), 11);
        assert_eq!(package.remotes.len(), 1);
        println!("{:#?}", package.remotes);
        match &package.remotes["gsm=gilead-biostats/gsm@v2.2.2"] {
            (name, PackageRemote::Git { url, .. }) => {
                assert_eq!(url, "https://github.com/gilead-biostats/gsm");
                assert_eq!(name, &Some("gsm".to_string()));
            }
            _ => panic!("Should have gotten a git repo"),
        }
    }

    #[test]
    fn can_read_version() {
        let version = parse_version("src/tests/descriptions/gsm.app.DESCRIPTION").unwrap();
        assert_eq!(version.original, "2.3.0.9000");
    }
}
