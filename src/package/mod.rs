use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use toml_edit::{InlineTable, Value};

mod builtin;
mod description;
mod parser;
mod remotes;
mod version;

use crate::consts::BASE_PACKAGES;
pub use builtin::{BuiltinPackages, get_builtin_versions_from_library};
pub use description::{parse_description_file, parse_description_file_in_folder, parse_version};
pub use parser::parse_package_file;
pub use remotes::PackageRemote;
pub use version::{Operator, Version, VersionRequirement, deserialize_version};

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, Encode, Decode, Serialize)]
#[serde(rename_all = "lowercase")]
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

#[derive(Debug, Hash, Eq, PartialEq, Clone, Encode, Decode, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dependency {
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
            Dependency::Pinned { requirement, .. } => Some(requirement),
        }
    }

    pub(crate) fn as_toml_value(&self) -> Value {
        match self {
            Self::Simple(name) => Value::from(name.as_str()),
            Self::Pinned { name, requirement } => {
                let mut table = InlineTable::new();
                table.insert("name", Value::from(name.as_str()));
                table.insert("requirement", Value::from(&requirement.to_string()));
                Value::InlineTable(table)
            }
        }
    }
}

#[derive(Debug, Default, PartialEq, Clone, Encode, Decode)]
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
    recommended: bool,
    pub(crate) needs_compilation: bool,
    // {remote_string => (pkg name, remote)}
    pub(crate) remotes: HashMap<String, (Option<String>, PackageRemote)>,
}

#[derive(Debug, Default, PartialEq, Clone)]
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

        // The deps in linkingTo can be listed already in depends
        for dep in &self.linking_to {
            if out.iter().find(|x| x.name() == dep.name()).is_none() {
                out.push(dep);
            }
        }

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

/// Returns whether this folder contains compiled R files
pub fn is_binary_package(path: impl AsRef<Path>, name: &str) -> bool {
    path.as_ref().join("R").join(format!("{name}.rdx")).exists()
}
