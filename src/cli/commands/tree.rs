use crate::Context;
use crate::lockfile::Source;
use crate::package::PackageType;
use crate::{ResolvedDependency, UnresolvedDependency, Version};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, PartialEq, Copy, Clone)]
enum NodeKind {
    Normal,
    Last,
}

impl NodeKind {
    fn prefix(&self) -> &'static str {
        match self {
            NodeKind::Normal => "├─",
            NodeKind::Last => "└─",
        }
    }
}

fn child_kind(idx: usize, len: usize) -> NodeKind {
    if idx + 1 == len {
        NodeKind::Last
    } else {
        NodeKind::Normal
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub enum NodeState<'a> {
    Resolved {
        version: &'a Version,
        source: &'a Source,
        package_type: PackageType,
        ignored: bool,
    },
    Unresolved {
        error: Option<String>,
        version_req: Option<String>,
    },
}

#[derive(Debug, PartialEq, Serialize)]
pub struct TreeNode<'a> {
    name: &'a str,
    sys_deps: Option<&'a Vec<String>>,
    children: Vec<TreeNode<'a>>,
    state: NodeState<'a>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    is_duplicate: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    in_config: bool,
}

impl<'a> TreeNode<'a> {
    fn resolved(
        name: &'a str,
        dependency: &'a ResolvedDependency,
        sys_deps: Option<&'a Vec<String>>,
        children: Vec<TreeNode<'a>>,
    ) -> Self {
        Self {
            name,
            sys_deps,
            children,
            state: NodeState::Resolved {
                version: dependency.version.as_ref(),
                source: &dependency.source,
                package_type: dependency.kind,
                ignored: dependency.ignored,
            },
            is_duplicate: false,
            in_config: false,
        }
    }

    fn duplicate(
        name: &'a str,
        dependency: &'a ResolvedDependency,
        sys_deps: Option<&'a Vec<String>>,
    ) -> Self {
        let mut node = Self::resolved(name, dependency, sys_deps, vec![]);
        node.is_duplicate = true;
        node
    }

    fn unresolved(
        name: &'a str,
        unresolved: Option<&'a UnresolvedDependency>,
        sys_deps: Option<&'a Vec<String>>,
    ) -> Self {
        let (error, version_req) = if let Some(dep) = unresolved {
            (
                dep.error.clone(),
                dep.version_requirement.clone().map(|x| x.to_string()),
            )
        } else {
            (
                Some("unresolved dependency metadata missing".to_string()),
                None,
            )
        };

        Self {
            name,
            sys_deps,
            children: vec![],
            state: NodeState::Unresolved { error, version_req },
            is_duplicate: false,
            in_config: false,
        }
    }

    fn has_duplicate_descendant(&self) -> bool {
        self.is_duplicate || self.children.iter().any(Self::has_duplicate_descendant)
    }

    fn has_config_descendant(&self) -> bool {
        self.in_config || self.children.iter().any(Self::has_config_descendant)
    }

    /// Flags every node whose package is declared in the project config
    fn mark_config_deps(&mut self, config_deps: &HashSet<&str>) {
        self.in_config = config_deps.contains(self.name);
        for child in self.children.iter_mut() {
            child.mark_config_deps(config_deps);
        }
    }

    fn markers(&self) -> String {
        let mut out = String::new();
        if self.in_config {
            out.push_str(" (◆)");
        }
        if self.is_duplicate {
            out.push_str(" (*)");
        }
        out
    }

    fn get_sys_deps(&self, show_sys_deps: bool) -> String {
        if !show_sys_deps {
            return String::new();
        }

        if let Some(s) = self.sys_deps {
            if s.is_empty() {
                String::new()
            } else {
                format!(" (sys: {})", s.join(", "))
            }
        } else {
            String::new()
        }
    }

    fn get_details(&self, show_sys_deps: bool) -> String {
        let sys_deps = self.get_sys_deps(show_sys_deps);

        match &self.state {
            NodeState::Resolved {
                version,
                source,
                package_type,
                ignored,
            } => {
                if *ignored {
                    return "ignored".to_string();
                }

                let mut elems = vec![
                    format!("version: {version}"),
                    format!("source: {source}"),
                    format!("type: {package_type}"),
                ];

                if !sys_deps.is_empty() {
                    elems.push(format!("system deps: {sys_deps}"));
                }

                elems.join(", ")
            }
            NodeState::Unresolved { error, version_req } => {
                let mut elems = vec![String::from("unresolved")];
                if let Some(e) = error {
                    elems.push(format!("error: {e}"));
                }
                if let Some(v) = version_req {
                    elems.push(format!("version requirement: {v}"));
                }
                elems.join(", ")
            }
        }
    }

    fn print_recursive(
        &self,
        prefix: &str,
        kind: NodeKind,
        current_depth: usize,
        max_depth: Option<usize>,
        show_sys_deps: bool,
    ) {
        if let Some(d) = max_depth
            && current_depth > d
        {
            return;
        }

        println!(
            "{prefix}{} {} [{}]{}",
            kind.prefix(),
            self.name,
            self.get_details(show_sys_deps),
            self.markers()
        );

        if self.is_duplicate {
            return;
        }

        let child_prefix = match kind {
            NodeKind::Normal => &format!("{prefix}│ "),
            NodeKind::Last => &format!("{prefix}  "),
        };

        for (idx, child) in self.children.iter().enumerate() {
            child.print_recursive(
                child_prefix,
                child_kind(idx, self.children.len()),
                current_depth + 1,
                max_depth,
                show_sys_deps,
            );
        }
    }
}

fn unresolved_node<'d>(
    name: &'d str,
    unresolved_deps_by_name: &HashMap<&'d str, &'d UnresolvedDependency>,
    sys_deps: Option<&'d Vec<String>>,
) -> TreeNode<'d> {
    TreeNode::unresolved(name, unresolved_deps_by_name.get(name).copied(), sys_deps)
}

fn recursive_finder<'d>(
    name: &'d str,
    deps_by_name: &HashMap<&'d str, &'d ResolvedDependency>,
    unresolved_deps_by_name: &HashMap<&'d str, &'d UnresolvedDependency>,
    context: &'d Context,
    ancestors: &mut Vec<&'d str>,
    visited: &mut HashSet<&'d str>,
) -> TreeNode<'d> {
    if ancestors.contains(&name) {
        if let Some(resolved) = deps_by_name.get(name) {
            return TreeNode::resolved(
                name,
                resolved,
                context.system_dependencies.get(name),
                vec![],
            );
        }
        return unresolved_node(
            name,
            unresolved_deps_by_name,
            context.system_dependencies.get(name),
        );
    }

    if visited.contains(name)
        && let Some(resolved) = deps_by_name.get(name)
    {
        return TreeNode::duplicate(name, resolved, context.system_dependencies.get(name));
    }

    if let Some(resolved) = deps_by_name.get(name) {
        ancestors.push(name);
        let mut dep_names = resolved.all_dependencies_names();
        dep_names.sort_unstable();
        let children: Vec<_> = dep_names
            .into_iter()
            .map(|dep_name| {
                recursive_finder(
                    dep_name,
                    deps_by_name,
                    unresolved_deps_by_name,
                    context,
                    ancestors,
                    visited,
                )
            })
            .collect();
        ancestors.pop();
        visited.insert(name);

        TreeNode::resolved(
            name,
            resolved,
            context.system_dependencies.get(name),
            children,
        )
    } else {
        unresolved_node(
            name,
            unresolved_deps_by_name,
            context.system_dependencies.get(name),
        )
    }
}

#[derive(Debug, Serialize)]
pub struct Tree<'a> {
    nodes: Vec<TreeNode<'a>>,
}

impl Tree<'_> {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Every package declared in the project config that appears anywhere in the tree,
    /// deduplicated and sorted
    pub fn config_dependencies(&self) -> Vec<&str> {
        let mut names = Vec::new();
        let mut stack: Vec<&TreeNode> = self.nodes.iter().collect();
        while let Some(node) = stack.pop() {
            if node.in_config {
                names.push(node.name);
            }
            stack.extend(node.children.iter());
        }
        names.sort_unstable();
        names.dedup();
        names
    }

    pub fn print(&self, max_depth: Option<usize>, show_sys_deps: bool) {
        for (i, tree) in self.nodes.iter().enumerate() {
            println!(
                "▶ {} [{}]{}",
                tree.name,
                tree.get_details(show_sys_deps),
                tree.markers()
            );

            if !tree.is_duplicate {
                for (j, child) in tree.children.iter().enumerate() {
                    child.print_recursive(
                        "",
                        child_kind(j, tree.children.len()),
                        2,
                        max_depth,
                        show_sys_deps,
                    );
                }
            }

            if i + 1 < self.nodes.len() {
                println!();
            }
        }

        let has_config = self.nodes.iter().any(TreeNode::has_config_descendant);
        let has_duplicate = self.nodes.iter().any(TreeNode::has_duplicate_descendant);
        if has_config || has_duplicate {
            println!();
        }
        if has_config {
            println!("(◆) package declared in the project config");
        }
        if has_duplicate {
            println!("(*) dependency already shown above");
        }
    }
}

fn dependents_by_name<'d>(
    resolved_deps: &'d [ResolvedDependency],
) -> HashMap<&'d str, Vec<&'d ResolvedDependency<'d>>> {
    let mut out: HashMap<_, Vec<&'d ResolvedDependency>> = HashMap::new();
    for dep in resolved_deps {
        for name in dep.all_dependencies_names() {
            out.entry(name).or_default().push(dep);
        }
    }
    for dependents in out.values_mut() {
        dependents.sort_unstable_by_key(|d| d.name.as_ref());
    }
    out
}

fn recursive_dependent_finder<'d>(
    dep: &'d ResolvedDependency,
    dependents: &HashMap<&'d str, Vec<&'d ResolvedDependency<'d>>>,
    context: &'d Context,
    ancestors: &mut Vec<&'d str>,
    visited: &mut HashSet<&'d str>,
) -> TreeNode<'d> {
    let name = dep.name.as_ref();
    let sys_deps = context.system_dependencies.get(name);

    if ancestors.contains(&name) {
        return TreeNode::resolved(name, dep, sys_deps, vec![]);
    }

    if visited.contains(name) {
        return TreeNode::duplicate(name, dep, sys_deps);
    }
    visited.insert(name);

    ancestors.push(name);
    let children = dependents
        .get(name)
        .map(|found| {
            found
                .iter()
                .map(|d| recursive_dependent_finder(d, dependents, context, ancestors, visited))
                .collect()
        })
        .unwrap_or_default();
    ancestors.pop();

    TreeNode::resolved(name, dep, sys_deps, children)
}

fn inverted_tree<'d>(
    target: &str,
    resolved_deps: &'d [ResolvedDependency],
    unresolved_deps_by_name: &HashMap<&'d str, &'d UnresolvedDependency>,
    context: &'d Context,
) -> Vec<TreeNode<'d>> {
    let dependents = dependents_by_name(resolved_deps);
    let mut ancestors = Vec::new();
    let mut visited = HashSet::new();

    if let Some(dep) = resolved_deps.iter().find(|d| d.name.as_ref() == target) {
        return vec![recursive_dependent_finder(
            dep,
            &dependents,
            context,
            &mut ancestors,
            &mut visited,
        )];
    }

    // A package can be depended on without being resolved itself, so it still gets a tree
    let Some((name, unresolved)) = unresolved_deps_by_name.get_key_value(target) else {
        return Vec::new();
    };
    let mut node = TreeNode::unresolved(
        name,
        Some(unresolved),
        context.system_dependencies.get(*name),
    );
    visited.insert(*name);
    ancestors.push(*name);
    node.children = dependents
        .get(*name)
        .map(|found| {
            found
                .iter()
                .map(|d| {
                    recursive_dependent_finder(
                        d,
                        &dependents,
                        context,
                        &mut ancestors,
                        &mut visited,
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    vec![node]
}

pub fn tree<'a>(
    context: &'a Context,
    resolved_deps: &'a [ResolvedDependency],
    unresolved_deps: &'a [UnresolvedDependency],
    invert_target: Option<&str>,
) -> Tree<'a> {
    let unresolved_deps_by_name: HashMap<_, _> = unresolved_deps
        .iter()
        .map(|d| (d.name.as_ref(), d))
        .collect();

    if let Some(target) = invert_target {
        let config_deps: HashSet<&str> = context
            .config
            .dependencies()
            .iter()
            .map(|d| d.name())
            .collect();
        let mut nodes = inverted_tree(target, resolved_deps, &unresolved_deps_by_name, context);
        for node in nodes.iter_mut() {
            node.mark_config_deps(&config_deps);
        }
        return Tree { nodes };
    }

    let deps_by_name: HashMap<_, _> = resolved_deps.iter().map(|d| (d.name.as_ref(), d)).collect();
    let mut nodes = Vec::new();
    let mut visited: HashSet<&str> = HashSet::new();

    for top_level_dep in context.config.dependencies() {
        if let Some(found) = deps_by_name.get(top_level_dep.name()) {
            let name = found.name.as_ref();
            // Top-level deps are user-requested — always show their full subtree, even if it
            // was already encountered as a transitive dep of an earlier top-level.
            visited.remove(name);
            let mut ancestors = Vec::new();
            nodes.push(recursive_finder(
                name,
                &deps_by_name,
                &unresolved_deps_by_name,
                context,
                &mut ancestors,
                &mut visited,
            ));
        } else {
            nodes.push(unresolved_node(
                top_level_dep.name(),
                &unresolved_deps_by_name,
                context.system_dependencies.get(top_level_dep.name()),
            ));
        }
    }

    Tree { nodes }
}
