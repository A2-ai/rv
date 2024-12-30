use crate::ResolvedDependency;
use std::collections::{HashMap, VecDeque};

/// Groups the sorted set of dependencies to ensure that no package is installed in the same step
/// as its dependencies.
fn find_install_steps<'a>(
    deps: &'a [ResolvedDependency<'a>],
    sorted: Vec<&'a str>,
) -> Vec<Vec<&'a ResolvedDependency<'a>>> {
    let by_name: HashMap<_, _> = deps.iter().map(|d| (d.name, d)).collect();
    let mut install_steps = vec![Vec::new()];
    for dep_name in sorted {
        let mut push_new = false;
        if let Some(current) = install_steps.last_mut() {
            for d in &by_name[dep_name].dependencies {
                if current.contains(&by_name[d]) {
                    push_new = true;
                    break;
                }
            }
            if !push_new {
                current.push(by_name[dep_name]);
                continue;
            }
        }

        if push_new {
            install_steps.push(vec![by_name[dep_name]]);
        }
    }

    install_steps
}

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

/// We want to parallelize the installation process as much as possible.
/// For this we need to know in which order are things meant to be installed to avoid issues.
/// This function will take the list of resolved dependencies for a project (so all dependencies
/// are present) and we know that it _should_ be a DAG.
/// On top of sorting the dependencies, this will split them by steps they can be installed in
/// so no package is installed in the same step as any of its dependencies so it's safe to run
/// in parallel regardless of a dependency build time.
/// The output of this function is meant to run sequentially but the inner vecs should be run
/// concurrently.
pub(crate) fn get_install_plan<'a>(
    deps: &'a [ResolvedDependency<'a>],
) -> Vec<Vec<&'a ResolvedDependency<'a>>> {
    let sorted = topological_sort(deps);
    find_install_steps(deps, sorted)
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
    fn can_get_install_plan() {
        let mut deps = vec![
            get_resolved_dep("C", vec!["E"]),
            get_resolved_dep("D", vec!["F"]),
            get_resolved_dep("E", vec![]),
            get_resolved_dep("F", vec![]),
            get_resolved_dep("A", vec!["C", "D"]),
            get_resolved_dep("G", vec!["A", "F"]),
        ];

        // we expect:
        // (E, F) -> (C, D) -> (A) -> (G)
        let order = get_install_plan(&deps);
        assert_eq!(order.len(), 4);
        let just_names: Vec<Vec<_>> = order
            .iter()
            .map(|ord| ord.iter().map(|d| d.name).collect())
            .collect();

        assert_eq!(
            just_names,
            vec![vec!["E", "F"], vec!["C", "D"], vec!["A"], vec!["G"]]
        );
    }
}
