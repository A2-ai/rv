use std::cmp::Ordering;
use std::fs::File;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::{fmt, fs, io};

use anyhow::{bail, Result};
use etcetera::BaseStrategy;
use serde::{Deserialize, Serialize};

use crate::consts::{ADDITIONAL_R_VERSIONS_FILENAME, DEFAULT_R_PATHS};
use crate::{RCmd, RCommandLine};

#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
pub enum Operator {
    Equal,
    Greater,
    Lower,
    GreaterOrEqual,
    LowerOrEqual,
}

impl fmt::Display for Operator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = match self {
            Self::Equal => "==",
            Self::Greater => ">",
            Self::Lower => "<",
            Self::GreaterOrEqual => ">=",
            Self::LowerOrEqual => "<=",
        };

        write!(f, "{}", c)
    }
}

impl FromStr for Operator {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "==" => Ok(Self::Equal),
            ">" => Ok(Self::Greater),
            "<" => Ok(Self::Lower),
            ">=" => Ok(Self::GreaterOrEqual),
            "<=" => Ok(Self::LowerOrEqual),
            _ => todo!("Handle error: {s}"),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Version {
    // TODO: pack versions in a u64 for faster comparison if needed
    // I don't think a package has more than 10 values in their version
    parts: [u32; 10],
    pub original: String,
}

impl Version {
    /// Returns the major/minor part of a version.
    /// Only meant to be used for R itself.
    // unlikely to be a problem but if hashing on the list is too slow but we can return a u64 instead
    // realistically R is going to be at 4.5 so we would be safe with a u8 or u16 even
    #[inline]
    pub fn major_minor(&self) -> [u32; 2] {
        [self.parts[0], self.parts[1]]
    }

    /// This function is meant to take an R version specified within a config and find it on the system
    /// This allows the binaries built by rv to be built by the correct version of R
    pub fn find_r_version_command(&self) -> Result<RCommandLine> {
        // Give preference to the R version on the $PATH
        if self.does_r_binary_match_version(PathBuf::from("R")) {
            return Ok(RCommandLine {
                r: PathBuf::from("R"),
            });
        }

        // look through all paths specified as default and whatever is the additional r path config file
        for mut path in potential_r_paths() {
            // if path is supposed to be an R binary, check it exists and that its version matches
            if !path.ends_with("*") {
                if !path.exists() {
                    continue;
                }
                if !self.does_r_binary_match_version(path.to_path_buf()) {
                    continue;
                }
                return Ok(RCommandLine { r: path });
            }
            // otherwise, remove the wildcard and ensure its parent exists
            path.pop();
            if !path.exists() {
                continue;
            }
            // look in each sub folder for "bin/R" and see that that R binary is the correct version
            let r_path = list_content(&path)
                .into_iter()
                .map(|p| p.join("bin/R"))
                .filter(|p| p.exists())
                .find(|p| self.does_r_binary_match_version(p.to_path_buf()));
            if let Some(r) = r_path {
                return Ok(RCommandLine { r });
            }
        }
        bail!(format!(
            "Could not find R version on system matching specified version ({self})"
        ))
    }

    // see if the found R binary matches the specified version.
    // If cannot determine version return false
    // Hazy matches version based on number of specified elements
    fn does_r_binary_match_version(&self, r_binary_path: PathBuf) -> bool {
        if let Ok(v) = (RCommandLine { r: r_binary_path }).version() {
            let num_specified = self.original.split('.').count();
            self.parts[..num_specified] == v.parts[..num_specified]
        } else {
            false
        }
    }
}

impl FromStr for Version {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts: Vec<u32> = s
            .trim()
            .replace('-', ".")
            .split('.')
            .map(|x| x.parse().unwrap())
            .collect();
        parts.resize(10, 0);

        Ok(Self {
            parts: parts.try_into().unwrap(),
            original: s.to_string(),
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.original)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl Eq for Version {}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn deserialize_version<'de, D>(deserializer: D) -> Result<Version, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: String = Deserialize::deserialize(deserializer)?;
    match Version::from_str(&v) {
        Ok(v) => Ok(v),
        Err(_) => Err(serde::de::Error::custom("Invalid version number")),
    }
}

// list the content within a directory as Vec<PathBuf>
// Used for wildcard R version directories
fn list_content(path: &PathBuf) -> Vec<PathBuf> {
    if let Ok(entries) = fs::read_dir(path) {
        entries
            .into_iter()
            .filter_map(Result::ok)
            .map(|x| x.path())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    }
}

// determine the potential paths R may be on based on the list of default paths and paths specified within the r versions config
fn potential_r_paths() -> Vec<PathBuf> {
    // determine r versions config path based on XDG spec
    let config_file = etcetera::base_strategy::choose_base_strategy()
        .map(|s| s.config_dir().join(ADDITIONAL_R_VERSIONS_FILENAME))
        .unwrap_or(PathBuf::new());

    // if path doesn't exist, return only the default r paths
    if !config_file.exists() {
        return DEFAULT_R_PATHS
            .into_iter()
            .map(PathBuf::from)
            .collect::<Vec<_>>();
    }

    // if path does exist, read the content of the file and append the default r versions to it
    // paths specified in config file are given precedent
    let mut content = if let Ok(file) = File::open(config_file) {
        let reader = io::BufReader::new(file);
        reader
            .lines()
            .filter_map(|line| {
                line.ok()
                    .map(PathBuf::from)
                    .and_then(|path| fs::canonicalize(&path).ok())
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    content.extend(DEFAULT_R_PATHS.into_iter().map(PathBuf::from));
    content
}

/// A package can require specific version for some versions.
/// Most of the time it's using >= but there are also some
/// >, <, <= here and there and a couple of ==
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct VersionRequirement {
    pub(crate) version: Version,
    op: Operator,
}

impl VersionRequirement {
    pub fn is_satisfied(&self, version: &Version) -> bool {
        match self.op {
            Operator::Equal => &self.version == version,
            Operator::Greater => version > &self.version,
            Operator::Lower => version < &self.version,
            Operator::GreaterOrEqual => version >= &self.version,
            Operator::LowerOrEqual => version <= &self.version,
        }
    }

    pub fn new(version: Version, op: Operator) -> Self {
        Self { version, op }
    }
}

impl FromStr for VersionRequirement {
    type Err = ();

    // s is for format `(>= 4.5)`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut current = String::new();
        let mut version = None;
        let mut op = None;

        for c in s.trim().chars() {
            if c == '(' {
                continue;
            }
            if c == ' ' {
                // we should have the op in current
                // however formatting across lines can sometimes cause multiple whitespaces
                // after the op like "(>=   1.2.0)"
                // so if we hit more whitespace after setting the op we can just continue
                if op.is_none() {
                    op = Some(Operator::from_str(&current).expect("TODO"));
                    current = String::new();
                }
                continue;
            }
            if c == ')' {
                version = Some(Version::from_str(&current).expect("TODO"));
                continue;
            }
            current.push(c);
        }

        Ok(Self {
            version: version.unwrap(),
            op: op.unwrap(),
        })
    }
}

impl fmt::Display for VersionRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({} {})", self.op, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn can_parse_pinning_strings() {
        let inputs = vec![
            "(> 1.0.0)",
            "(>= 1.0)",
            "(>=    1.0)", // extra whitespace
            "(== 1.7-7-1)",
            "(<= 2023.8.2.1)",
            "(< 1.0-10)",
            "(>= 1.98-1.16)",
        ];
        // Just making sure we don't panic on weird but existing versions
        for input in inputs {
            println!("{:?}", VersionRequirement::from_str(input));
        }
    }

    #[test]
    fn can_parse_cran_versions() {
        let inputs = vec![
            "1.0.0",
            "1.0",
            "1.7-7-1",
            "2023.8.2.1",
            "1.0-10",
            "0.0.0.9",
            "2024.11.29",
            "2019.10-1",
            "1.0.2.1000",
            "1.98-1.16",
            "1.0.5.2.1",
            "4041.111",
            "1.0.0-1.1.2",
            "3.7-0",
        ];
        // Just making sure we don't panic on weird but existing versions
        for input in inputs {
            println!("{:?}", Version::from_str(input).unwrap());
        }
    }

    #[test]
    fn can_parse_version_requirements() {
        assert_eq!(
            VersionRequirement::from_str("(== 1.0.0)")
                .unwrap()
                .to_string(),
            "(== 1.0.0)"
        );
    }

    #[test]
    fn can_compare_versions() {
        assert!(Version::from_str("1.0").unwrap() == Version::from_str("1.0.0").unwrap());
        assert!(Version::from_str("1.1").unwrap() > Version::from_str("1.0.0").unwrap());
    }

    #[test]
    fn can_get_minor_major() {
        assert_eq!(Version::from_str("1.0").unwrap().major_minor(), [1, 0]);
        assert_eq!(Version::from_str("1.0.0").unwrap().major_minor(), [1, 0]);
        assert_eq!(Version::from_str("4.5").unwrap().major_minor(), [4, 5]);
    }
}
