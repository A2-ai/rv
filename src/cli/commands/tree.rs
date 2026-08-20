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
        }
    }

    fn has_duplicate_descendant(&self) -> bool {
        self.is_duplicate || self.children.iter().any(Self::has_duplicate_descendant)
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

        let dup_marker = if self.is_duplicate { " (*)" } else { "" };
        println!(
            "{prefix}{} {} [{}]{dup_marker}",
            kind.prefix(),
            self.name,
            self.get_details(show_sys_deps)
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
    pub fn print(&self, max_depth: Option<usize>, show_sys_deps: bool) {
        for (i, tree) in self.nodes.iter().enumerate() {
            let dup_marker = if tree.is_duplicate { " (*)" } else { "" };
            println!(
                "▶ {} [{}]{dup_marker}",
                tree.name,
                tree.get_details(show_sys_deps)
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

        if self.nodes.iter().any(TreeNode::has_duplicate_descendant) {
            println!();
            println!("(*) dependency already shown above");
        }
    }
}

/// Builds inverted tree showing which top-level dependencies depend on the target package
fn build_inverted_tree<'a>(
    original_trees: Vec<TreeNode<'a>>,
    target_package: &'a str,
    deps_by_name: &HashMap<&'a str, &'a ResolvedDependency>,
    unresolved_deps_by_name: &HashMap<&'a str, &'a UnresolvedDependency>,
    context: &'a Context,
) -> Vec<TreeNode<'a>> {
    let mut inverted_trees = Vec::new();

    // For each top-level dependency, check if it has the target package in its tree
    for top_level_tree in original_trees {
        if let Some(inverted_subtree) = find_and_invert_paths(
            &top_level_tree,
            target_package,
            deps_by_name,
            unresolved_deps_by_name,
            context,
        ) {
            // Create tree with top-level as root and target as child
            let mut top_level_node = top_level_tree;
            top_level_node.children = vec![inverted_subtree];
            inverted_trees.push(top_level_node);
        }
    }

    inverted_trees
}

/// Finds the target package in a tree and builds inverted paths from target back to dependents
fn find_and_invert_paths<'a>(
    node: &TreeNode<'a>,
    target_package: &'a str,
    deps_by_name: &HashMap<&'a str, &'a ResolvedDependency>,
    unresolved_deps_by_name: &HashMap<&'a str, &'a UnresolvedDependency>,
    context: &'a Context,
) -> Option<TreeNode<'a>> {
    // If this node is the target, create target node with inverted dependencies
    if node.name == target_package {
        return Some(create_target_node_with_dependents(
            target_package,
            deps_by_name,
            unresolved_deps_by_name,
            context,
        ));
    }

    // Otherwise, recursively check children
    for child in &node.children {
        if let Some(inverted_child) = find_and_invert_paths(
            child,
            target_package,
            deps_by_name,
            unresolved_deps_by_name,
            context,
        ) {
            return Some(inverted_child);
        }
    }

    None
}

/// Creates a target node with its dependents as children (inverted dependencies)
fn create_target_node_with_dependents<'a>(
    target_package: &'a str,
    deps_by_name: &HashMap<&'a str, &'a ResolvedDependency>,
    unresolved_deps_by_name: &HashMap<&'a str, &'a UnresolvedDependency>,
    context: &'a Context,
) -> TreeNode<'a> {
    // Find all packages that directly depend on the target
    let mut dependents = Vec::new();

    for (name, dep) in deps_by_name {
        if dep.all_dependencies_names().contains(&target_package) {
            // Only include this dependent if it's not a different top-level dependency
            // We need to know which top-level we're building for, so we'll get it from the context
            // For now, we'll need to pass this information differently
            let mut visited = HashSet::new();
            visited.insert(target_package);
            let dependent_node = build_dependent_chain_with_cycle_detection(
                name,
                target_package,
                "", // We'll fix this by restructuring the function calls
                deps_by_name,
                context,
                &mut visited,
            );
            dependents.push(dependent_node);
        }
    }

    // Create the target node with dependents as children
    let sys_deps = context.system_dependencies.get(target_package);
    if let Some(dep) = deps_by_name.get(target_package).copied() {
        TreeNode::resolved(target_package, dep, sys_deps, dependents)
    } else {
        let mut node = TreeNode::unresolved(
            target_package,
            unresolved_deps_by_name.get(target_package).copied(),
            sys_deps,
        );
        node.children = dependents;
        node
    }
}

/// Builds a chain of dependents from a package that depends on the target with cycle detection
fn build_dependent_chain_with_cycle_detection<'a>(
    package_name: &'a str,
    target_package: &'a str,
    current_top_level: &'a str,
    deps_by_name: &HashMap<&'a str, &'a ResolvedDependency>,
    context: &'a Context,
    visited: &mut HashSet<&'a str>,
) -> TreeNode<'a> {
    let dep = deps_by_name[&package_name];

    // Add this package to visited set
    visited.insert(package_name);

    // Find packages that depend on this package (but not ones we've already visited)
    let mut higher_dependents = Vec::new();
    for (name, higher_dep) in deps_by_name {
        if *name != target_package
            && !visited.contains(name)
            && higher_dep.all_dependencies_names().contains(&package_name)
        {
            // Only continue if this is the current top-level dependency we're building for
            // or if it's not a top-level dependency at all
            if *name == current_top_level || !is_top_level_dependency(name, context) {
                let higher_dependent_node = build_dependent_chain_with_cycle_detection(
                    name,
                    target_package,
                    current_top_level,
                    deps_by_name,
                    context,
                    visited,
                );
                higher_dependents.push(higher_dependent_node);
            }
        }
    }

    // Remove this package from visited set (backtrack)
    visited.remove(package_name);

    TreeNode::resolved(
        package_name,
        dep,
        context.system_dependencies.get(package_name),
        higher_dependents,
    )
}

/// Helper function to check if a package is a top-level dependency
fn is_top_level_dependency(package_name: &str, context: &Context) -> bool {
    context
        .config
        .dependencies()
        .iter()
        .any(|dep| dep.name() == package_name)
}

pub fn tree<'a>(
    context: &'a Context,
    resolved_deps: &'a [ResolvedDependency],
    unresolved_deps: &'a [UnresolvedDependency],
    invert_target: Option<&'a str>,
) -> Tree<'a> {
    let deps_by_name: HashMap<_, _> = resolved_deps.iter().map(|d| (d.name.as_ref(), d)).collect();
    let unresolved_deps_by_name: HashMap<_, _> = unresolved_deps
        .iter()
        .map(|d| (d.name.as_ref(), d))
        .collect();

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

    let nodes = if let Some(target_package) = invert_target {
        build_inverted_tree(
            nodes,
            target_package,
            &deps_by_name,
            &unresolved_deps_by_name,
            context,
        )
    } else {
        nodes
    };

    Tree { nodes }
}
