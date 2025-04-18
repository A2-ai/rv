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

impl Resolution<'_> {
    pub fn finalize(&mut self) {
        let mut solver = DependencySolver::default();
        for package in &self.found {
            solver.add_package(&package.name, &package.version);
            for dep in &package.dependencies {
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
}
