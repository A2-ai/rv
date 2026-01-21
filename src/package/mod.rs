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

use crate::{consts::BASE_PACKAGES, git::url::GitUrl};
pub use builtin::{BuiltinPackages, get_builtin_versions_from_library};
pub use description::{parse_description_file, parse_description_file_in_folder, parse_version};
pub use parser::{parse_dependencies, parse_package_file};
pub use remotes::PackageRemote;
pub use version::{Operator, Version, VersionRequirement, deserialize_version, serialize_version};

pub(crate) use remotes::parse_remote;

const COMPILED_R_SUBDIRS: [&str; 2] = ["R", "data"];

#[derive(Debug, PartialEq, Eq, Copy, Clone, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Hash, Eq, PartialEq, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: Version,
    pub r_requirement: Option<VersionRequirement>,
    pub depends: Vec<Dependency>,
    pub imports: Vec<Dependency>,
    pub suggests: Vec<Dependency>,
    pub enhances: Vec<Dependency>,
    pub linking_to: Vec<Dependency>,
    pub(crate) license: String,
    pub md5_sum: String,
    pub(crate) path: Option<String>,
    pub(crate) recommended: bool,
    pub(crate) needs_compilation: bool,
    // {remote_string => (pkg name, remote)}
    pub(crate) remotes: HashMap<String, (Option<String>, PackageRemote)>,
    // The below fields are populated when packages are built from Git by tools like R-Universe
    // Used to install packages from R-Universe and sets us up to start editing the DESCRIPTION
    // file upon installations for compatibility with `sessioninfo`
    pub(crate) remote_url: Option<GitUrl>,
    pub(crate) remote_sha: Option<String>,
    pub(crate) remote_subdir: Option<String>,
    // The built field only exists when a package is a binary
    // https://rstudio.github.io/r-manuals/r-ints/Package-Structure.html
    pub(crate) built: Option<String>,
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

    pub fn dependencies_to_install(
        &self,
        install_suggestions: bool,
    ) -> InstallationDependencies<'_> {
        let mut out = Vec::with_capacity(30);
        // TODO: consider if this should be an option or just take it as an empty vector otherwise
        out.extend(self.depends.iter());
        out.extend(self.imports.iter());

        // The deps in linkingTo can be listed already in depends
        for dep in &self.linking_to {
            if !out.iter().any(|x| x.name() == dep.name()) {
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
/// Error only occurs if the DESCRIPTION file cannot be parsed
pub fn is_binary_package(
    path: impl AsRef<Path>,
    name: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let path = path.as_ref();

    // If the package does not have a parse-able DESCRIPTION file, the entire package is not valid and we should not try to install it
    let pkg = parse_description_file_in_folder(path)?;

    // The only way for a package to contain <package name>.rdx in either the R or data subdirectories is if it has been built
    // While most packages DO contain either R code or data, that is not required, therefore we check more robustly
    let name_rdx = format!("{name}.rdx");
    for subdir in COMPILED_R_SUBDIRS {
        if path.join(subdir).join(&name_rdx).exists() {
            return Ok(true);
        }
    }

    // If the package does not have the `Built` field, inserted by R during a build, then we know it is not a binary
    if pkg.built.is_none() {
        return Ok(false);
    }

    // Built packages will have a `Meta` directory with a `package.rds` file. If not, we know it is not a binary
    // https://rstudio.github.io/r-manuals/r-ints/Package-Structure.html#metadata
    if !path.join("Meta").join("package.rds").exists() {
        return Ok(false);
    }

    // If the package has both the `Built` field of a DESCRIPTION file and `Meta/package.rds`, then we will say it is a binary
    // even if we do not find any .rdx files
    Ok(true)
}
