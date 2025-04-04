#![allow(missing_docs)]
use crate::consts::{BASE_PACKAGES, RECOMMENDED_PACKAGES};
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

mod description;
mod parser;
mod remotes;
mod version;

pub use description::{parse_description_file, parse_description_file_in_folder, parse_version};
pub use parser::{parse_dependencies, parse_package_file};
pub use remotes::PackageRemote;
pub use version::{deserialize_version, Operator, Version, VersionRequirement};

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, Encode, Decode)]
pub enum PackageType {
    Source,
    Binary,
}

impl fmt::Display for PackageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source => write!(f, "source"),
            Self::Binary => write!(f, "binary"),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Encode, Decode, Serialize, Deserialize)]
pub enum Dependency {
    Simple(String),
    Pinned {
        name: String,
        requirement: VersionRequirement,
    },
}

impl Dependency {
    pub fn name(&self) -> &str {
        match self {
            Dependency::Simple(s) => s,
            Dependency::Pinned { name, .. } => name,
        }
    }

    pub fn version_requirement(&self) -> Option<&VersionRequirement> {
        match self {
            Dependency::Simple(_) => None,
            Dependency::Pinned {
                ref requirement, ..
            } => Some(requirement),
        }
    }
}

#[derive(Debug, Default, PartialEq, Clone, Encode, Decode)]
/// Used to decode the PACKAGES file and the package's DESCRIPTION file
pub struct Package {
    pub name: String,
    pub version: Version,
    pub r_requirement: Option<VersionRequirement>,
    pub depends: Vec<Dependency>,
    pub imports: Vec<Dependency>,
    pub suggests: Vec<Dependency>,
    pub enhances: Vec<Dependency>,
    pub linking_to: Vec<Dependency>,
    pub license: String,
    pub md5_sum: String,
    pub path: Option<String>,
    pub recommended: bool,
    pub needs_compilation: bool,
    pub remotes: HashMap<String, (Option<String>, PackageRemote)>,
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
                .filter(|p| {
                    !BASE_PACKAGES.contains(&p.name()) && !RECOMMENDED_PACKAGES.contains(&p.name())
                })
                .collect()
        } else {
            Vec::new()
        };

        InstallationDependencies {
            direct: out
                .into_iter()
                .filter(|p| {
                    !BASE_PACKAGES.contains(&p.name()) && !RECOMMENDED_PACKAGES.contains(&p.name())
                })
                .collect(),
            suggests,
        }
    }
}

pub fn is_binary_package(path: impl AsRef<Path>, name: &str) -> bool {
    path.as_ref().join("R").join(format!("{name}.rdx")).exists()
}
