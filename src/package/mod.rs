use std::collections::HashMap;
use std::fmt;

use crate::consts::BASE_PACKAGES;
use serde::{Deserialize, Serialize};

mod description;
mod parser;
mod remotes;
mod version;

pub use description::parse_description_file_in_folder;
pub use parser::parse_package_file;
pub use remotes::PackageRemote;
pub use version::{deserialize_version, Version, VersionRequirement};

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, Serialize, Deserialize)]
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
    recommended: bool,
    pub(crate) needs_compilation: bool,
    // {remote_string => (pkg name, remote)}
    pub(crate) remotes: HashMap<String, (Option<String>, PackageRemote)>,
    pub(crate) remote_url: Option<String>,
    pub(crate) remote_sha: Option<String>,
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

    // pub fn invalid_remotes(&self) -> bool {
    //     let mut issues = Vec::new();
    //
    //     for (original, (name, remote)) in &self.remotes {
    //
    //     }
    // }
}
