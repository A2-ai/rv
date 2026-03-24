use crate::Version;
use crate::consts::DESCRIPTION_FILENAME;
use crate::package::Package;
use crate::package::parser::parse_package_file;
use std::fs;
use std::fs::File;
use std::io::BufRead;
use std::path::Path;
use std::str::FromStr;

/// A DESCRIPTION file is like a PACKAGE file, only that it contains info about a single package
pub fn parse_description_file(content: &str) -> Option<Package> {
    // TODO: handle remotes in package for deps
    let new_content = content.to_string() + "\n";

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

/// Parse a DESCRIPTION file into all raw key-value pairs, preserving insertion order.
///
/// Returns a `Vec<(key, value)>` with every field from the file in order. Multi-line
/// continuation values (lines starting with whitespace) are collapsed into a single
/// space-separated string.
///
/// All values are returned as plain strings — no structural parsing is done. In
/// particular, comma-separated dependency fields like `Depends`, `Imports`,
/// `Suggests`, `Enhances`, and `LinkingTo` are returned as-is (e.g.
/// `"R (>= 3.6), dplyr, rlang (>= 1.0)"`). Callers that need these as arrays
/// should split on `,` and trim whitespace.
///
/// Handles keys with special characters (`Authors@R`, `Config/testthat/edition`)
/// and fields where the value starts on the next line (`Depends:\n    R (>= 4.0)`).
pub fn parse_description_fields(content: &str) -> Vec<(String, String)> {
    let mut fields: Vec<(String, String)> = Vec::new();

    for line in content.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation line — append to the current field's value
            if let Some((_key, value)) = fields.last_mut() {
                if !value.is_empty() {
                    value.push(' ');
                }
                value.push_str(line.trim());
            }
        } else if let Some(colon_pos) = line.find(':') {
            let key = &line[..colon_pos];
            let value = line[colon_pos + 1..].trim();
            fields.push((key.to_string(), value.to_string()));
        }
        // Blank lines or lines without a colon and no leading whitespace are skipped
    }

    fields
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
                assert_eq!(url.url(), "https://github.com/gilead-biostats/gsm");
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

    #[test]
    fn fields_simple() {
        let content = "Package: R6\nVersion: 2.6.1\nTitle: Encapsulated Classes\n";
        let fields = parse_description_fields(content);
        assert_eq!(fields[0], ("Package".into(), "R6".into()));
        assert_eq!(fields[1], ("Version".into(), "2.6.1".into()));
        assert_eq!(fields[2], ("Title".into(), "Encapsulated Classes".into()));
    }

    #[test]
    fn fields_multiline_continuation() {
        let content = "Package: R6\nDescription: A long description\n    that spans multiple\n    lines\nVersion: 1.0\n";
        let fields = parse_description_fields(content);
        assert_eq!(fields[0].0, "Package");
        assert_eq!(
            fields[1],
            (
                "Description".into(),
                "A long description that spans multiple lines".into()
            )
        );
        assert_eq!(fields[2], ("Version".into(), "1.0".into()));
    }

    #[test]
    fn fields_special_keys() {
        let content = "Package: R6\nAuthors@R: person(\"A\")\nConfig/testthat/edition: 3\n";
        let fields = parse_description_fields(content);
        assert_eq!(fields[1], ("Authors@R".into(), "person(\"A\")".into()));
        assert_eq!(fields[2], ("Config/testthat/edition".into(), "3".into()));
    }

    #[test]
    fn fields_preserves_insertion_order() {
        let content = "Zebra: z\nAlpha: a\nMiddle: m\n";
        let fields = parse_description_fields(content);
        let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["Zebra", "Alpha", "Middle"]);
    }

    #[test]
    fn fields_empty_key_line_with_continuation() {
        // Depends: has no value on the key line, value is entirely on continuation lines
        let content = "Package: test\nDepends:\n    R (>= 4.0)\nImports:\n    bslib,\n    dplyr\n";
        let fields = parse_description_fields(content);
        let depends = fields.iter().find(|(k, _)| k == "Depends").unwrap();
        assert_eq!(depends.1, "R (>= 4.0)");
        let imports = fields.iter().find(|(k, _)| k == "Imports").unwrap();
        assert_eq!(imports.1, "bslib, dplyr");
    }

    #[test]
    fn fields_mid_expression_line_break() {
        // Version constraint split across lines: pkg (\n    >= 3.0)
        let content = "Package: test\nDepends: R (>= 3.5.0)\nImports: cli (>= 3.0.0), dplyr (>=\n        1.0.5), glue (>=\n     1.6.0), rlang\n";
        let fields = parse_description_fields(content);
        let imports = fields.iter().find(|(k, _)| k == "Imports").unwrap();
        assert_eq!(
            imports.1,
            "cli (>= 3.0.0), dplyr (>= 1.0.5), glue (>= 1.6.0), rlang"
        );
    }

    #[test]
    fn fields_real_gsm_app_description() {
        let content = fs::read_to_string("src/tests/descriptions/gsm.app.DESCRIPTION").unwrap();
        let fields = parse_description_fields(&content);
        let depends = fields.iter().find(|(k, _)| k == "Depends").unwrap();
        assert_eq!(depends.1, "R (>= 4.0)");
        let imports = fields.iter().find(|(k, _)| k == "Imports").unwrap();
        // Should be a comma-separated list of packages
        assert!(imports.1.contains("bslib"));
        assert!(imports.1.contains("dplyr"));
    }
}
