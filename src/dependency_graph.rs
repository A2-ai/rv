use std::collections::{HashMap, HashSet, VecDeque};

use crate::ResolvedDependency;

/// Returns the topological sort for the given set of dependencies (assuming it's a DAG)
/// https://en.wikipedia.org/wiki/Topological_sorting
/// Uses Kahn's algorithm
fn topological_sort<'a>(deps: &'a [ResolvedDependency<'a>]) -> Vec<&'a str> {
    // number of unmet dependency for each deps
    let mut in_degree = HashMap::new();
    let mut dependents = HashMap::new();
    let mut sorted = Vec::new();

    // Each time we see a dependency as one of other dependency, we increase its in_degree
    for dep in deps {
        *in_degree.entry(dep.name).or_insert(0) += dep.dependencies.len();

        for subdep in &dep.dependencies {
            dependents
                .entry(*subdep)
                .or_insert_with(Vec::new)
                .push(dep.name);
        }
    }

    // Find the first batch that can be installed immediately
    let mut queue: VecDeque<_> = in_degree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(d, _)| d)
        .cloned()
        .collect();

    while let Some(dep_name) = queue.pop_front() {
        sorted.push(dep_name);

        if let Some(dependents) = dependents.get(dep_name) {
            for dep in dependents {
                if let Some(degree) = in_degree.get_mut(dep) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dep);
                    }
                }
            }
        }
    }

    sorted
}

#[derive(Debug, PartialEq)]
pub enum BuildStep<'a> {
    Install(&'a ResolvedDependency<'a>),
    Wait,
    Done,
}

#[derive(Debug)]
pub struct BuildPlan<'a> {
    deps: &'a [ResolvedDependency<'a>],
    sorted: Vec<&'a str>,
    installed: HashSet<String>,
    installing: HashSet<&'a str>,
    /// Full list of dependencies for each dependencies.
    /// The value will be updated as packages are installed to remove them from that list
    full_deps: HashMap<&'a str, HashSet<&'a str>>,
}

impl<'a> BuildPlan<'a> {
    pub fn new(deps: &'a [ResolvedDependency<'a>]) -> Self {
        let sorted = topological_sort(deps);
        let by_name: HashMap<_, _> = deps.iter().map(|d| (d.name, d)).collect();
        let mut full_deps = HashMap::new();

        for dep in deps {
            let mut all_deps = HashSet::new();

            let mut queue = VecDeque::from_iter(dep.dependencies.iter());
            while let Some(dep_name) = queue.pop_front() {
                all_deps.insert(*dep_name);
                for d in &by_name[dep_name].dependencies {
                    if !all_deps.contains(d) {
                        queue.push_back(d);
                    }
                }
            }

            full_deps.insert(dep.name, all_deps);
        }

        Self {
            deps,
            sorted,
            full_deps,
            installed: HashSet::new(),
            installing: HashSet::new(),
        }
    }

    pub fn mark_installed(&mut self, name: &str) {
        self.installed.insert(name.to_string());
        self.installing.remove(name);

        for (_, deps) in self.full_deps.iter_mut() {
            deps.remove(name);
        }
    }

    fn is_skippable(&self, name: &str) -> bool {
        self.installed.contains(name) || self.installing.contains(name)
    }

    fn is_done(&self) -> bool {
        self.installed.len() == self.deps.len()
    }

    /// get a package to install, an enum {Package, Wait, Done}
    pub fn get(&mut self) -> BuildStep {
        if self.installed.len() == self.deps.len() {
            return BuildStep::Done;
        }

        for dep in &self.sorted {
            // Skip the ones being installed or already installed
            if self.is_skippable(dep) {
                continue;
            }

            // Then we check whether all the deps are already installed
            if self.full_deps[dep].is_empty() {
                self.installing.insert(dep);
                return BuildStep::Install(self.deps.iter().find(|d| d.name == *dep).unwrap());
            }
        }

        BuildStep::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PackageType;

    fn get_resolved_dep<'a>(name: &'a str, dependencies: Vec<&'a str>) -> ResolvedDependency<'a> {
        ResolvedDependency {
            name,
            dependencies,
            version: "",
            repository: "",
            needs_compilation: false,
            kind: PackageType::Source,
        }
    }

    #[test]
    fn can_get_install_plan2() {
        let deps = vec![
            get_resolved_dep("C", vec!["E"]),
            get_resolved_dep("D", vec!["F"]),
            get_resolved_dep("E", vec![]),
            get_resolved_dep("F", vec![]),
            get_resolved_dep("A", vec!["C", "D"]),
            get_resolved_dep("G", vec!["A", "F"]),
            get_resolved_dep("J", vec![]),
        ];

        // we would normally expect:
        // (E, F, J) -> (C, D) -> (A) -> (G)
        // but let's imagine J will be super slow. We can install all the rest in the meantime
        let mut plan = BuildPlan::new(&deps);
        // Pretend we are already installing J
        plan.installing.insert("J");
        // Now it should be E or F twice
        let step = plan.get();
        assert!(vec![BuildStep::Install(&deps[2]), BuildStep::Install(&deps[3])].contains(&step));
        let step = plan.get();
        assert!(vec![BuildStep::Install(&deps[2]), BuildStep::Install(&deps[3])].contains(&step));
        assert_eq!(plan.installing, HashSet::from_iter(["J", "E", "F"]));
        // Now we should be stuck with Waiting since all other packages depend on those 3
        assert_eq!(plan.get(), BuildStep::Wait);
        assert_eq!(plan.get(), BuildStep::Wait);
        // Let's mark E as installed, it should get C to install next
        plan.mark_installed("E");
        assert_eq!(plan.get(), BuildStep::Install(&deps[0]));
        // now we're stuck again
        assert_eq!(plan.get(), BuildStep::Wait);
        // Let's mark F as installed, it should get D to install next
        plan.mark_installed("F");
        assert_eq!(plan.get(), BuildStep::Install(&deps[1]));
        // We mark C and D as installed, we should get A next
        plan.mark_installed("C");
        plan.mark_installed("D");
        assert_eq!(plan.get(), BuildStep::Install(&deps[4]));
        plan.mark_installed("A");
        // we should get G now
        assert_eq!(plan.get(), BuildStep::Install(&deps[5]));
        plan.mark_installed("G");

        // Only J is left but we are left hanging
        assert_eq!(plan.get(), BuildStep::Wait);
        // finally mark it as done and we should be done
        plan.mark_installed("J");
        assert_eq!(plan.get(), BuildStep::Done);
    }
}
