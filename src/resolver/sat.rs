use crate::{Version, VersionRequirement};
use std::collections::HashMap;

/// Literals in CNF formula are represented as positive or negative integers
type Literal = i32;
/// A clause is a disjunction of literals
type Clause = Vec<Literal>;
/// A formula in CNF is a conjunction of clauses
type Formula = Vec<Clause>;

#[derive(Debug, Clone, PartialEq)]
struct Package<'d> {
    name: &'d str,
    version: &'d Version,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackageRequirement<'d> {
    pub package: &'d str,
    pub requirement: &'d VersionRequirement,
    pub required_by: &'d str,
}

/// A SAT solver
/// This assumes we have found all the dependencies, it should not be called if we are missing
/// anything. It's only called if there are version requirements.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct DependencySolver<'d> {
    packages: HashMap<&'d str, Vec<Package<'d>>>,
    requirements: Vec<PackageRequirement<'d>>,
}

impl<'d> DependencySolver<'d> {
    pub fn add_package(&mut self, name: &'d str, version: &'d Version) {
        if let Some(packages) = self.packages.get_mut(name) {
            for p in packages.iter() {
                if p.name == name && p.version == version {
                    return;
                }
            }
            packages.push(Package { name, version });
        } else {
            self.packages.insert(name, vec![Package { name, version }]);
        }
    }

    pub fn add_requirement(
        &mut self,
        package: &'d str,
        requirement: &'d VersionRequirement,
        required_by: &'d str,
    ) {
        self.requirements.push(PackageRequirement {
            package,
            requirement,
            required_by,
        });
    }

    // We map a tuple (pkg name, version) to an incrementing integer
    fn get_variable_mappings(&self) -> HashMap<(&'d str, &'d Version), Literal> {
        let mut pkg_version_to_var = HashMap::new();
        let mut var = 1;

        for (pkg_name, pkg_list) in &self.packages {
            for pkg in pkg_list {
                pkg_version_to_var.insert((*pkg_name, pkg.version), var);
                var += 1;
            }
        }

        pkg_version_to_var
    }

    fn create_clauses(
        &self,
        pkg_version_to_var: &HashMap<(&'d str, &'d Version), Literal>,
    ) -> (Formula, HashMap<usize, usize>) {
        let mut clauses = Vec::new();
        let mut clauses_to_req = HashMap::new();

        // Add clauses to ensure each package has at most one version selected
        for (name, packages) in &self.packages {
            let mut all_versions: Vec<_> = packages.iter().map(|p| p.version).collect();
            all_versions.sort();
            all_versions.dedup();

            // For each pair of versions, add a clause that at least one must be False
            // If there's only one version, no clauses will be added
            for (i, &v1) in all_versions.iter().enumerate() {
                for &v2 in all_versions.iter().skip(i + 1) {
                    if let (Some(&var1), Some(&var2)) = (
                        pkg_version_to_var.get(&(name, v1)),
                        pkg_version_to_var.get(&(name, v2)),
                    ) {
                        clauses.push(vec![-var1, -var2]);
                    }
                }
            }
        }

        // Now handle the version requirements
        for (i, req) in self.requirements.iter().enumerate() {
            // For each version of the requiring package that's selected,
            // at least one satisfying version of the required package must be selected
            let mut satisfying_required_vars = Vec::new();

            // Find all versions of the required package that satisfy the requirement
            if let Some(pkgs) = self.packages.get(req.package) {
                for required_pkg in pkgs {
                    if req.requirement.is_satisfied(required_pkg.version) {
                        if let Some(&required_var) =
                            pkg_version_to_var.get(&(required_pkg.name, required_pkg.version))
                        {
                            satisfying_required_vars.push(required_var);
                        }
                    }
                }
            }

            // If no version satisfies the requirement, mark the requirement as unsatisfiable
            if satisfying_required_vars.is_empty() {
                // Add an empty clause to make the formula unsatisfiable
                clauses.push(Vec::new());
                clauses_to_req.insert(clauses.len() - 1, i);
                continue;
            }

            // Otherwise, at least one of the satisfying versions must be selected
            clauses.push(satisfying_required_vars);
            clauses_to_req.insert(clauses.len() - 1, i);
        }

        (clauses, clauses_to_req)
    }

    /// Check whether a formula is satisfied by the given assignment
    fn is_satisfied(&self, formula: &Formula, assignment: &HashMap<Literal, bool>) -> bool {
        for clause in formula {
            // A clause is satisfied if at least one literal is True
            let mut satisfied = false;

            for &literal in clause {
                let var = literal.abs();

                // If the variable is not assigned, the clause is not definitely satisfied
                if !assignment.contains_key(&var) {
                    continue;
                }
                let value = assignment[&var];

                // If the literal is positive, it's satisfied when var is True
                // If the literal is negative, it's satisfied when var is False
                let literal_satisfied = (literal > 0 && value) || (literal < 0 && !value);

                if literal_satisfied {
                    satisfied = true;
                    break;
                }
            }

            // If any clause is not satisfied, the whole formula is not satisfied
            if !satisfied {
                return false;
            }
        }

        true
    }

    fn solve_sat_recursive(
        &self,
        formula: &Formula,
        assignment: &mut HashMap<Literal, bool>,
        var_index: i32,
        num_vars: i32,
    ) -> bool {
        // Quick check for empty clauses - formula is unsatisfiable
        if formula.iter().any(|clause| clause.is_empty()) {
            return false;
        }

        // If all variables have been assigned, check if formula is satisfied
        if var_index > num_vars {
            return self.is_satisfied(formula, assignment);
        }

        // Try assigning True to current variable
        assignment.insert(var_index, true);
        if self.solve_sat_recursive(formula, assignment, var_index + 1, num_vars) {
            return true;
        }

        // Try assigning False to current variable
        assignment.insert(var_index, false);
        if self.solve_sat_recursive(formula, assignment, var_index + 1, num_vars) {
            return true;
        }

        // Backtrack: remove the current variable assignment
        assignment.remove(&var_index);
        false
    }

    fn solve_sat(&self, formula: &Formula, num_vars: i32) -> HashMap<Literal, bool> {
        let mut assignment = HashMap::new();
        if self.solve_sat_recursive(formula, &mut assignment, 1, num_vars) {
            assignment
        } else {
            HashMap::new()
        }
    }

    /// This will run the SAT solver multiple times while removing clauses to see which ones are
    /// actually the ones causing the issues
    fn find_minimal_unsatisfiable_subset(
        &self,
        clauses: &Formula,
        clauses_to_req: &HashMap<usize, usize>,
    ) -> Vec<PackageRequirement<'d>> {
        // Start with all clauses related to requirements
        let mut current_clauses: Vec<(usize, &Clause)> = clauses
            .iter()
            .enumerate()
            .filter(|(i, _)| clauses_to_req.contains_key(i))
            .collect();

        // Try to remove clauses one by one, keeping the formula unsatisfiable
        let mut i = 0;
        while i < current_clauses.len() {
            // Create a subset without the current clause
            let test_clauses: Vec<Clause> = current_clauses
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, (_, clause))| (*clause).clone())
                .collect();

            // Add back the non-requirement clauses
            let mut all_test_clauses = test_clauses;
            for (idx, clause) in clauses.iter().enumerate() {
                if !clauses_to_req.contains_key(&idx) {
                    all_test_clauses.push(clause.clone());
                }
            }

            // Check if still unsatisfiable
            let num_vars = self.packages.values().fold(0, |acc, pkgs| acc + pkgs.len()) as i32;
            if self.solve_sat(&all_test_clauses, num_vars).is_empty() {
                // Still unsatisfiable, we can remove this clause from our MUS
                current_clauses.remove(i);
            } else {
                // Became satisfiable, this clause is necessary for unsatisfiability
                i += 1;
            }
        }

        // Convert clause indices back to requirements
        current_clauses
            .iter()
            .filter_map(|(idx, _)| {
                clauses_to_req
                    .get(idx)
                    .map(|req_idx| self.requirements[*req_idx].clone())
            })
            .collect()
    }

    fn find_failed_requirements(
        &self,
        clauses: &Formula,
        clauses_to_req: &HashMap<usize, usize>,
    ) -> Vec<PackageRequirement<'d>> {
        // Collect all empty clauses (requirements that can't be satisfied)
        let mut unsatisfiable_reqs = Vec::new();
        for (i, clause) in clauses.iter().enumerate() {
            if clause.is_empty() {
                if let Some(req_index) = clauses_to_req.get(&i) {
                    unsatisfiable_reqs.push(self.requirements[*req_index].clone());
                }
            }
        }

        // If we found any directly unsatisfiable requirements, return them
        if !unsatisfiable_reqs.is_empty() {
            return unsatisfiable_reqs;
        }

        // If we are here, it means we have multiple conflicting clauses
        // We can remove each requirement and try to resolve it
        self.find_minimal_unsatisfiable_subset(clauses, clauses_to_req)
    }

    pub fn solve(&self) -> Result<HashMap<&'d str, &'d Version>, Vec<PackageRequirement<'d>>> {
        log::debug!(
            "Solving dependencies for {} packages and {} version requirements",
            self.packages.len(),
            self.requirements.len()
        );
        let pkg_version_to_var = self.get_variable_mappings();
        let (clauses, clauses_to_req) = self.create_clauses(&pkg_version_to_var);

        let var_to_pkg_version: HashMap<_, _> =
            pkg_version_to_var.iter().map(|(k, v)| (v, k)).collect();

        log::debug!("Starting SAT solving");
        let assignment = self.solve_sat(&clauses, var_to_pkg_version.len() as i32);

        // No solution exists
        if assignment.is_empty() {
            let errs = self.find_failed_requirements(&clauses, &clauses_to_req);
            return Err(errs);
        }

        // Convert the SAT solution to package versions
        let mut solution = HashMap::new();
        for (var, value) in assignment {
            if value {
                let (pkg, version) = &var_to_pkg_version[&var];
                solution.insert(*pkg, *version);
            }
        }

        Ok(solution)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn get_resolver<'a>(
        packages: &'a [(&'static str, Version)],
        requirements: &'a [(&'static str, VersionRequirement, &'static str)],
    ) -> DependencySolver<'a> {
        let mut resolver = DependencySolver::default();
        for (name, version) in packages {
            resolver.add_package(name, version);
        }

        for (name, req, required_by) in requirements {
            resolver.add_requirement(name, &req, required_by);
        }

        resolver
    }

    #[test]
    fn no_version_req_ok() {
        let packages = vec![
            ("A", Version::from_str("1.0.0").unwrap()),
            ("B", Version::from_str("1.1.0").unwrap()),
        ];
        let resolver = get_resolver(&packages, &[]);
        let result = resolver.solve().unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn same_pkg_diff_version_no_req() {
        let packages = vec![
            ("A", Version::from_str("1.0.0").unwrap()),
            ("A", Version::from_str("1.1.0").unwrap()),
        ];
        let resolver = get_resolver(&packages, &[]);
        let result = resolver.solve().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["A"].original, "1.0.0");
    }

    #[test]
    fn same_pkg_same_version_ok() {
        let packages = vec![
            ("A", Version::from_str("1.0.0").unwrap()),
            ("A", Version::from_str("1.0.0").unwrap()),
        ];
        let resolver = get_resolver(&packages, &[]);
        let result = resolver.solve().unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result["A"].original, "1.0.0");
    }

    #[test]
    fn version_req_ok() {
        let packages = vec![
            ("A", Version::from_str("1.0.0").unwrap()),
            ("A", Version::from_str("2.0.0").unwrap()),
            ("B", Version::from_str("1.1.0").unwrap()),
        ];
        let requirements = vec![(
            "A",
            VersionRequirement::from_str("(>= 2.0.0)").unwrap(),
            "B",
        )];
        let resolver = get_resolver(&packages, &requirements);
        let result = resolver.solve().unwrap();
        assert_eq!(result.len(), 2);
        // It will pick the second since there is a version required and it's the only one matching
        assert_eq!(result["A"], &packages[1].1);
    }

    #[test]
    fn version_req_error() {
        let packages = vec![
            ("A", Version::from_str("1.0.0").unwrap()),
            ("B", Version::from_str("1.1.0").unwrap()),
        ];
        let requirements = vec![(
            "A",
            VersionRequirement::from_str("(>= 2.0.0)").unwrap(),
            "B",
        )];
        let resolver = get_resolver(&packages, &requirements);
        let result = resolver.solve().unwrap_err();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].package, "A");
        assert_eq!(result[0].requirement.to_string(), "(>= 2.0.0)");
        assert_eq!(result[0].required_by, "B");
    }

    #[test]
    fn version_req_conflict() {
        let packages = vec![
            ("A", Version::from_str("2.5.0").unwrap()),
            ("B", Version::from_str("1.1.0").unwrap()),
            ("C", Version::from_str("1.1.0").unwrap()),
        ];
        let requirements = vec![
            ("A", VersionRequirement::from_str("(> 3.0.0)").unwrap(), "B"),
            ("A", VersionRequirement::from_str("(< 2.0.0)").unwrap(), "C"),
        ];
        let resolver = get_resolver(&packages, &requirements);
        let result = resolver.solve().unwrap_err();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn multiple_version_req_conflict() {
        let packages = vec![
            ("A", Version::from_str("2.0.0").unwrap()),
            ("B", Version::from_str("1.1.0").unwrap()),
            ("C", Version::from_str("1.1.0").unwrap()),
            ("D", Version::from_str("3.1.0").unwrap()),
        ];
        let requirements = vec![
            ("A", VersionRequirement::from_str("(> 2.0.0)").unwrap(), "B"),
            ("A", VersionRequirement::from_str("(< 2.0.0)").unwrap(), "C"),
            (
                "D",
                VersionRequirement::from_str("(>= 3.1.0)").unwrap(),
                "B",
            ),
            ("D", VersionRequirement::from_str("(< 3.1.0)").unwrap(), "C"),
        ];
        let resolver = get_resolver(&packages, &requirements);
        let result = resolver.solve().unwrap_err();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn multiple_satisfied_version_conflict() {
        let packages = vec![
            ("A", Version::from_str("1.0.0").unwrap()),
            ("A", Version::from_str("2.0.0").unwrap()),
            ("C", Version::from_str("1.1.0").unwrap()),
            ("D", Version::from_str("1.1.0").unwrap()),
        ];
        let requirements = vec![
            (
                "A",
                VersionRequirement::from_str("(== 2.0.0)").unwrap(),
                "B",
            ),
            (
                "A",
                VersionRequirement::from_str("(== 1.0.0)").unwrap(),
                "C",
            ),
        ];
        let resolver = get_resolver(&packages, &requirements);
        let result = resolver.solve().unwrap_err();
        assert_eq!(result.len(), 2);
    }
}
