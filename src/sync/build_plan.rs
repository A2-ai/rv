use std::collections::{HashMap, HashSet, VecDeque};

use crate::lockfile::Source;
use crate::{ResolvedDependency, Version};

#[derive(Debug, PartialEq)]
pub enum BuildStep<'a> {
    Install(&'a ResolvedDependency<'a>),
    Wait,
    Done,
}

#[derive(Debug)]
pub struct BuildPlan<'a> {
    deps: &'a [ResolvedDependency<'a>],
    pub(crate) installed: HashSet<&'a str>,
    pub(crate) installing: HashSet<&'a str>,
    /// Full list of dependencies for each dependencies.
    /// The value will be updated as packages are installed to remove them from that list
    pub(crate) full_deps: HashMap<&'a str, HashSet<&'a str>>,
}

impl<'a> BuildPlan<'a> {
    pub fn new(deps: &'a [ResolvedDependency<'a>]) -> Self {
        let by_name: HashMap<_, _> = deps.iter().map(|d| (d.name.as_ref(), d)).collect();
        let mut full_deps = HashMap::new();

        for dep in deps {
            if dep.ignored {
                continue;
            }
            let mut all_deps = HashSet::new();

            let mut queue = VecDeque::from_iter(dep.dependencies.iter().map(|x| x.name()));
            while let Some(dep_name) = queue.pop_front() {
                all_deps.insert(dep_name);
                for d in &by_name[dep_name].dependencies {
                    if !all_deps.contains(d.name()) {
                        queue.push_back(d.name());
                    }
                }
            }

            full_deps.insert(dep.name.as_ref(), all_deps);
        }

        Self {
            deps,
            full_deps,
            installed: HashSet::new(),
            installing: HashSet::new(),
        }
    }

    pub fn mark_installed(&mut self, name: &str) {
        // The lifetime for the name might be different from that struct
        let pkg = self
            .deps
            .iter()
            .find(|d| d.name == name)
            .expect("to find the dep");
        self.installed.insert(pkg.name.as_ref());
        self.installing.remove(pkg.name.as_ref());

        for (_, deps) in self.full_deps.iter_mut() {
            deps.remove(pkg.name.as_ref());
        }
    }

    fn is_skippable(&self, name: &str) -> bool {
        self.installed.contains(name) || self.installing.contains(name)
    }

    fn is_done(&self) -> bool {
        self.installed.len() == self.deps().len()
    }

    fn deps(&self) -> Vec<&'a ResolvedDependency<'a>> {
        self.deps.iter().filter(|x| !x.ignored).collect()
    }

    pub fn num_to_install(&self) -> usize {
        self.deps().len() - self.installed.len()
    }

    pub fn all_dependencies(&self) -> HashMap<&str, (&Version, &Source)> {
        self.deps()
            .iter()
            .map(|r| (r.name.as_ref(), (r.version.as_ref(), &r.source)))
            .collect()
    }

    /// get a package to install, an enum {Package, Wait, Done}
    pub fn get(&mut self) -> BuildStep<'_> {
        if self.is_done() {
            return BuildStep::Done;
        }

        for (dep, _) in self.full_deps.iter().filter(|(_, v)| v.is_empty()) {
            // Skip the ones being installed or already installed
            if self.is_skippable(dep) {
                continue;
            }
            self.installing.insert(dep);
            return BuildStep::Install(
                self.deps
                    .iter()
                    .find(|d| d.name == *dep)
                    .expect("it should have a dep with that name"),
            );
        }

        BuildStep::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::InstallationStatus;
    use crate::lockfile::Source;
    use crate::package::{Dependency, PackageType};
    use std::borrow::Cow;
    use std::str::FromStr;
    use url::Url;

    fn get_resolved_dep<'a>(name: &'a str, dependencies: Vec<&'a str>) -> ResolvedDependency<'a> {
        ResolvedDependency {
            name: Cow::from(name),
            dependencies: dependencies
                .into_iter()
                .map(|x| Cow::Owned(Dependency::Simple(x.to_string())))
                .collect(),
            suggests: Vec::new(),
            version: Cow::Owned(Version::from_str("0.1.0").unwrap()),
            source: Source::Repository {
                repository: Url::parse("https://something.com").unwrap(),
            },
            install_suggests: false,
            force_source: false,
            kind: PackageType::Source,
            installation_status: InstallationStatus::Binary(false),
            path: None,
            from_lockfile: false,
            from_remote: false,
            remotes: HashMap::new(),
            local_resolved_path: None,
            env_vars: HashMap::new(),
            ignored: false,
        }
    }

    #[test]
    fn can_get_install_plan() {
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
        assert!([BuildStep::Install(&deps[2]), BuildStep::Install(&deps[3])].contains(&step));
        let step = plan.get();
        assert!([BuildStep::Install(&deps[2]), BuildStep::Install(&deps[3])].contains(&step));
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
        // Calling it again doesn't change anything
        assert_eq!(plan.get(), BuildStep::Done);
    }
}
