use crate::config::Dependency;
use crate::repository::RepositoryDatabase;
use crate::version::Version;
use core::fmt;
use std::collections::{HashSet, VecDeque};
use std::fmt::Formatter;
use std::str::FromStr;
use crate::package::PackageType;

#[derive(Debug, PartialEq, Clone)]
struct ResolvedDependency<'d> {
    name: &'d str,
    version: &'d str,
    repository: &'d str,
    dependencies: Vec<&'d str>,
    needs_compilation: bool,
    kind: PackageType,
}

impl<'a> fmt::Display for ResolvedDependency<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}={} (from {}, type={})",
            self.name, self.version, self.repository, self.kind
        )
    }
}

#[derive(Debug, Default, PartialEq)]
struct Resolver<'d> {
    // The repositories are stored in the order defined in the config
    // The last should get priority over previous repositories
    repositories: &'d [RepositoryDatabase],
}

impl<'d> Resolver<'d> {
    pub fn new(repositories: &'d [RepositoryDatabase]) -> Self {
        Self { repositories }
    }

    // TODO: handle missing dependencies
    fn resolve(
        &self,
        r_version: &str,
        dependencies: &'d [Dependency],
    ) -> Vec<ResolvedDependency<'d>> {
        let r_version = Version::from_str(r_version).expect("TODO");
        let mut out = Vec::new();
        let mut found = HashSet::with_capacity(dependencies.len() * 10);

        let mut queue: VecDeque<_> = dependencies
            .iter()
            .map(|d| {
                (
                    d.name(),
                    d.repository(),
                    d.install_suggestions(),
                    d.force_source(),
                )
            })
            .collect();

        while let Some((name, repository, install_suggestions, force_source)) = queue.pop_front() {
            // If we have already found that dependency, skip it
            // TODO: maybe different version req? we can cross that bridge later
            if found.contains(name) {
                continue;
            }

            for repo in self.repositories.iter().rev() {
                if let Some(r) = repository {
                    if repo.name != r {
                        continue;
                    }
                }

                if let Some((package, package_type)) =
                    repo.find_package(name, &r_version, force_source)
                {
                    found.insert(name);
                    let all_dependencies = package.dependencies_to_install(install_suggestions);
                    out.push(ResolvedDependency {
                        name: &package.name,
                        version: &package.version.original,
                        repository: &repo.name,
                        dependencies: all_dependencies.iter().map(|d| d.name()).collect(),
                        needs_compilation: package.needs_compilation,
                        kind: package_type,
                    });

                    for d in all_dependencies {
                        if !found.contains(d.name()) {
                            queue.push_back((d.name(), None, false, false));
                        }
                    }
                    break;
                }
            }

            if !found.contains(name) {
                panic!("Package {name} not found");
            }
        }

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::package::parse_package_file;
    use crate::repository::RepositoryDatabase;

    #[test]
    fn can_resolve_various_dependencies() {
        let paths = std::fs::read_dir("src/tests/resolution/").unwrap();
        let mut repositories = Vec::new();

        for (name, (src_filename, binary_filename)) in vec![
            ("test", ("posit-src.PACKAGE", Some("cran-binary.PACKAGE"))),
            ("gh-mirror", ("gh-pkg-mirror.PACKAGE", None)),
        ] {
            let content =
                std::fs::read_to_string(format!("src/tests/package_files/{src_filename}")).unwrap();
            let source_packages = parse_package_file(&content);
            let mut repository = RepositoryDatabase::new(name);
            repository.parse_source(&content);
            if let Some(bin) = binary_filename {
                let content =
                    std::fs::read_to_string(format!("src/tests/package_files/{bin}")).unwrap();
                repository.parse_binary(&content, "4.4.2");
            }
            repositories.push(repository);
        }

        let resolver = Resolver::new(&repositories);
        for path in paths {
            let p = path.unwrap().path();
            let config = Config::from_file(&p);
            let res = resolver.resolve("4.4.2", &config.project.dependencies);
            let mut out = String::new();
            for d in res {
                out.push_str(&d.to_string());
                out.push_str("\n");
            }
            // Output has been compared with pkgr for the same PACKAGE file
            insta::assert_snapshot!(p.file_name().unwrap().to_string_lossy().to_string(), out);
        }
    }
}
