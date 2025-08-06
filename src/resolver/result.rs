use crate::resolver::sat::DependencySolver;
use crate::{ResolvedDependency, UnresolvedDependency};
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct RequirementFailure {
    required_by: String,
    version_req: String,
}

impl fmt::Display for RequirementFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} requires {}", self.required_by, self.version_req)
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Resolution<'d> {
    pub found: Vec<ResolvedDependency<'d>>,
    pub failed: Vec<UnresolvedDependency<'d>>,
    pub req_failures: HashMap<String, Vec<RequirementFailure>>,
}

impl<'d> Resolution<'d> {
    pub fn add_found(&mut self, dep: ResolvedDependency<'d>) {
        for f in &self.found {
            if f == &dep {
                return;
            }
        }
        self.found.push(dep);
    }

    /// If we already found something matching, we skip trying to get it.
    /// This is only called when we would look up a repo for a package without a version requirement
    pub(crate) fn found_in_repo(&self, name: &str) -> bool {
        self.found
            .iter()
            .any(|d| d.source.is_repo() && d.name == name)
    }

    pub(crate) fn ignore(&mut self, name: &str) {
        if let Some(dep) = self.found.iter_mut().find(|dep| dep.name == name) {
            dep.ignored = true;
        }
    }

    pub fn finalize(&mut self) {
        // First we go through the failed dependencies to see if something that would match was found
        // (for example it can happen if someone puts a dep in a git package and specify that dep
        // directly in rproject.toml instead of remotes)
        let mut actually_found = HashSet::new();
        for (i, failed) in self.failed.iter().enumerate() {
            for pkg in &self.found {
                if pkg.name == failed.name {
                    if let Some(req) = &failed.version_requirement {
                        if req.is_satisfied(&pkg.version) {
                            actually_found.insert(i);
                        }
                    } else {
                        actually_found.insert(i);
                    }
                }
            }
        }
        let mut actually_found = actually_found.into_iter().collect::<Vec<_>>();
        actually_found.sort_unstable_by(|a, b| b.cmp(a));
        for i in actually_found {
            self.failed.remove(i);
        }

        let mut solver = DependencySolver::default();
        for package in &self.found {
            if package.ignored {
                continue;
            }
            solver.add_package(&package.name, &package.version);

            let deps = package.dependencies.iter().chain({
                if package.install_suggests {
                    package.suggests.iter()
                } else {
                    [].iter()
                }
            });

            for dep in deps {
                if let Some(req) = dep.version_requirement() {
                    solver.add_requirement(dep.name(), req, &package.name);
                }
            }
        }

        // If we have a different number of packages that means we have
        match solver.solve() {
            Ok(assignments) => {
                let mut names = HashSet::new();
                let mut indices = HashSet::new();
                for (i, pkg) in self.found.iter().enumerate() {
                    if names.contains(&pkg.name) {
                        continue;
                    }
                    if let Some(version) = assignments.get(pkg.name.as_ref()) {
                        if pkg.version.as_ref() == *version {
                            names.insert(&pkg.name);
                            indices.insert(i);
                        }
                    } else if pkg.ignored {
                        // We still insert ignored packages
                        names.insert(&pkg.name);
                        indices.insert(i);
                    }
                }

                drop(assignments);
                let mut current_idx = 0;
                self.found.retain(|_| {
                    let keep = indices.contains(&current_idx);
                    current_idx += 1;
                    keep
                })
            }
            Err(req_errors) => {
                let mut out = HashMap::new();
                for req in req_errors {
                    out.entry(req.package.to_string())
                        .or_insert_with(Vec::new)
                        .push(RequirementFailure {
                            required_by: req.required_by.to_string(),
                            version_req: req.requirement.to_string(),
                        });
                }
                self.req_failures = out;
            }
        }
    }

    pub fn is_success(&self) -> bool {
        self.failed.is_empty() && self.req_failures.is_empty()
    }

    pub fn req_error_messages(&self) -> Vec<String> {
        self.req_failures
            .iter()
            .map(|(name, reqs)| {
                let versions_msg = self
                    .found
                    .iter()
                    .filter(|f| f.name == name.as_str())
                    .map(|x| format!("        * {} (from {})", x.version.original, x.source))
                    .collect::<Vec<_>>()
                    .join("\n");

                let reqs_msg = reqs
                    .iter()
                    .map(|x| x.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");

                if versions_msg.is_empty() {
                    format!("{}:\n  - {} and no versions were found", name, reqs_msg)
                } else {
                    format!(
                        "{}:\n  - {} and the following version(s) were found:\n{}",
                        name, reqs_msg, versions_msg
                    )
                }
            })
            .collect()
    }
}
