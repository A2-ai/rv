use bincode::{Decode, Encode};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use crate::consts::RECOMMENDED_PACKAGES;
use crate::git::url::GitUrl;
use crate::package::{Dependency, Package, PackageType, deserialize_version, parse_package_file};
use crate::package::{Version, VersionRequirement, parse_remote};

#[derive(Debug, Default, PartialEq, Clone, Decode, Encode)]
pub struct RepositoryDatabase {
    pub url: String,
    pub(crate) source_packages: HashMap<String, Vec<Package>>,
    // Binary will have a single package for each package, no multiple
    // depending on the R version but we keep the Vec so the resolver code can work
    // for both binary and source
    // But each major.minor R version will get different binary package database
    pub(crate) binary_packages: HashMap<[u32; 2], HashMap<String, Vec<Package>>>,
}

impl RepositoryDatabase {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            ..Default::default()
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, RepositoryDatabaseError> {
        let reader = BufReader::new(
            std::fs::File::open(path.as_ref()).map_err(RepositoryDatabaseError::from_io)?,
        );

        bincode::decode_from_reader(reader, bincode::config::standard())
            .map_err(RepositoryDatabaseError::from_bincode)
    }

    pub fn persist(&self, path: impl AsRef<Path>) -> Result<(), RepositoryDatabaseError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(RepositoryDatabaseError::from_io)?;
        }
        let mut writer = BufWriter::new(
            std::fs::File::create(path.as_ref()).map_err(RepositoryDatabaseError::from_io)?,
        );
        bincode::encode_into_std_write(self, &mut writer, bincode::config::standard())
            .expect("valid data");

        Ok(())
    }

    pub fn parse_source(&mut self, content: &str) {
        self.source_packages = parse_package_file(content);
    }

    pub fn parse_binary(&mut self, content: &str, r_version: [u32; 2]) {
        let packages = parse_package_file(content);
        self.binary_packages.insert(r_version, packages);
    }

    pub fn parse_runiverse_api(&mut self, content: &str) {
        self.source_packages = parse_runiverse_api_file(content)
            .into_iter()
            .map(|(pkg_name, pkg)| (pkg_name, vec![pkg.into()]))
            .collect();
    }

    // We always prefer binary unless `force_source` is set to true
    pub fn find_package<'a>(
        &'a self,
        name: &str,
        version_requirement: Option<&VersionRequirement>,
        r_version: &Version,
        force_source: bool,
    ) -> Option<(&'a Package, PackageType)> {
        let find_package = |db: &'a HashMap<String, Vec<Package>>| -> Option<&'a Package> {
            // If we find multiple packages matching the requirement, we grab the one with the
            // highest R requirement matching the provided R version.
            // The list of packages is in the same order as in the PACKAGE file so we start
            // from the end since latter entries have priority
            db.get(name).and_then(|packages| {
                let mut max_r_version = None;
                let mut found = None;

                for p in packages.iter().rev() {
                    if !p.works_with_r_version(r_version) {
                        continue;
                    }

                    if let Some(req) = version_requirement
                        && !req.is_satisfied(&p.version)
                    {
                        continue;
                    }

                    match (max_r_version, p.r_requirement.as_ref()) {
                        (Some(_), None) => (),
                        (None, Some(v)) => {
                            max_r_version = Some(&v.version);
                            found = Some(p);
                        }
                        (Some(v1), Some(v2)) => {
                            if &v2.version > v1 {
                                max_r_version = Some(&v2.version);
                                found = Some(p);
                            }
                        }
                        (None, None) => found = Some(p),
                    }
                }

                found
            })
        };

        if !force_source
            && let Some(db) = self.binary_packages.get(&r_version.major_minor())
            && let Some(package) = find_package(db)
        {
            return Some((package, PackageType::Binary));
        }

        find_package(&self.source_packages).map(|p| (p, PackageType::Source))
    }

    pub(crate) fn get_binary_count(&self, r_version: &[u32; 2]) -> usize {
        self.binary_packages
            .get(r_version)
            .map(|db| db.len())
            .unwrap_or_default()
    }

    pub(crate) fn get_source_count(&self) -> usize {
        self.source_packages.len()
    }
}

fn yes_no_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.as_str() {
        "Yes" | "yes" => Ok(true),
        "No" | "no" => Ok(false),
        other => Err(serde::de::Error::custom(format!(
            "expected 'Yes' or 'No', got '{}'",
            other
        ))),
    }
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RUniversePackage {
    package: String,
    #[serde(deserialize_with = "deserialize_version")]
    version: Version,
    license: String,
    #[serde(rename = "MD5sum")]
    md5_sum: String,
    #[serde(deserialize_with = "yes_no_to_bool")]
    needs_compilation: bool,
    #[serde(default)]
    remotes: Vec<String>,
    #[serde(rename = "_dependencies", default)]
    dependencies: Vec<RUniverseDependency>,
    remote_url: GitUrl,
    remote_sha: String,
    remote_subdir: Option<String>,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
struct RUniverseDependency {
    package: String,
    version: Option<String>,
    role: Role,
}

#[derive(Debug, PartialEq, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum Role {
    Depends,
    Imports,
    Suggests,
    LinkingTo,
    Enhances,
}

fn parse_runiverse_api_file(content: &str) -> HashMap<String, RUniversePackage> {
    let apis: Vec<RUniversePackage> = serde_json::from_str(content).unwrap();
    let mut map = HashMap::new();
    for api in apis {
        map.insert(api.package.to_string(), api);
    }
    map
}

impl From<RUniversePackage> for Package {
    fn from(pkg: RUniversePackage) -> Self {
        fn map_dependencies(deps: &[RUniverseDependency], role: Role) -> Vec<Dependency> {
            deps.iter()
                .filter(|d| d.role == role && d.package != "R")
                .map(|d| {
                    if let Some(v) = &d.version {
                        let requirement = format!("({v})")
                            .parse::<VersionRequirement>()
                            .expect("Properly formatted version requirement");
                        Dependency::Pinned {
                            name: d.package.to_string(),
                            requirement,
                        }
                    } else {
                        Dependency::Simple(d.package.to_string())
                    }
                })
                .collect()
        }

        let mut remotes = HashMap::new();
        for remote in pkg.remotes.iter() {
            let (name_opt, parsed_remote) = parse_remote(remote);
            remotes.insert(remote.clone(), (name_opt, parsed_remote));
        }

        let r_requirement = pkg.dependencies.iter().find_map(|d| match &d.version {
            Some(ver) if d.package == "R" => Some(
                format!("({ver})")
                    .parse::<VersionRequirement>()
                    .expect("Properly formatted version requirement"),
            ),
            _ => None,
        });

        let recommended = RECOMMENDED_PACKAGES.contains(&pkg.package.as_str());

        Self {
            name: pkg.package,
            version: pkg.version,
            r_requirement,
            depends: map_dependencies(&pkg.dependencies, Role::Depends),
            imports: map_dependencies(&pkg.dependencies, Role::Imports),
            suggests: map_dependencies(&pkg.dependencies, Role::Suggests),
            enhances: map_dependencies(&pkg.dependencies, Role::Enhances),
            linking_to: map_dependencies(&pkg.dependencies, Role::LinkingTo),
            license: pkg.license,
            md5_sum: pkg.md5_sum,
            path: None,
            recommended,
            needs_compilation: pkg.needs_compilation,
            remotes,
            remote_url: Some(pkg.remote_url),
            remote_sha: Some(pkg.remote_sha),
            remote_subdir: pkg.remote_subdir,
            built: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to load package database")]
#[non_exhaustive]
pub struct RepositoryDatabaseError {
    pub source: RepositoryDatabaseErrorKind,
}

impl RepositoryDatabaseError {
    fn from_io(err: std::io::Error) -> Self {
        Self {
            source: RepositoryDatabaseErrorKind::Io(err),
        }
    }

    fn from_bincode(err: bincode::error::DecodeError) -> Self {
        Self {
            source: RepositoryDatabaseErrorKind::Bincode(err),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum RepositoryDatabaseErrorKind {
    Io(#[from] std::io::Error),
    Bincode(#[from] bincode::error::DecodeError),
}

#[cfg(test)]
mod test {
    use std::fs;

    use crate::RepositoryDatabase;

    #[test]
    fn test_r_universe_api_parse() {
        let mut runiverse_db = RepositoryDatabase::new("http://r-universe.dev");
        let content = fs::read_to_string("src/tests/r_universe/a2-ai.api").unwrap();
        runiverse_db.parse_runiverse_api(&content);

        let mut repo_db = RepositoryDatabase::new("http://a2-ai");
        let content = fs::read_to_string("src/tests/package_files/a2-ai-universe.PACKAGE").unwrap();
        repo_db.parse_source(&content);

        let runiverse_pkgs = &runiverse_db.source_packages;
        let repo_pkgs = &repo_db.source_packages;

        assert_eq!(runiverse_pkgs.len(), repo_pkgs.len());

        for (name, runiverse_pkg_vec) in runiverse_pkgs {
            let repo_pkg_vec = repo_pkgs
                .get(name)
                .unwrap_or_else(|| panic!("Package {name} not found in repo_db"));
            assert_eq!(runiverse_pkg_vec.len(), repo_pkg_vec.len());

            for (runiverse_pkg, repo_pkg) in runiverse_pkg_vec.iter().zip(repo_pkg_vec.iter()) {
                assert_eq!(
                    runiverse_pkg.name, repo_pkg.name,
                    "Package name mismatch for {name}"
                );
                assert_eq!(
                    runiverse_pkg.version, repo_pkg.version,
                    "Version mismatch for {name}"
                );
                assert_eq!(
                    runiverse_pkg.depends, repo_pkg.depends,
                    "Depends mismatch for {name}"
                );
                assert_eq!(
                    runiverse_pkg.suggests, repo_pkg.suggests,
                    "Suggests mismatch for {name}"
                );
                assert_eq!(
                    runiverse_pkg.imports, repo_pkg.imports,
                    "Imports mismatch for {name}"
                );
                assert_eq!(
                    runiverse_pkg.enhances, repo_pkg.enhances,
                    "Enhances mismatch for {name}"
                );
                assert_eq!(
                    runiverse_pkg.linking_to, repo_pkg.linking_to,
                    "LinkingTo mismatch for {name}"
                );
                assert_eq!(
                    runiverse_pkg.needs_compilation, repo_pkg.needs_compilation,
                    "NeedsCompilation mismatch for {name}"
                );
            }
        }
    }
}
