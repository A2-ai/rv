use crate::cli::CliContext;
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

#[derive(Debug, PartialEq, Serialize)]
pub struct TreeNode<'a> {
    name: &'a str,
    version: Option<&'a Version>,
    source: Option<&'a Source>,
    package_type: Option<PackageType>,
    sys_deps: Option<&'a Vec<String>>,
    resolved: bool,
    error: Option<String>,
    version_req: Option<String>,
    children: Vec<TreeNode<'a>>,
    ignored: bool,
}

impl TreeNode<'_> {
    fn get_sys_deps(&self, show_sys_deps: bool) -> String {
        if show_sys_deps {
            if let Some(s) = self.sys_deps {
                if s.is_empty() {
                    String::new()
                } else {
                    format!(" (sys: {})", s.join(", "))
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    }

    fn get_details(&self, show_sys_deps: bool) -> String {
        let sys_deps = self.get_sys_deps(show_sys_deps);
        let mut elems = Vec::new();
        if self.resolved {
            if self.ignored {
                return "ignored".to_string();
            }
            elems.push(format!("version: {}", self.version.unwrap()));
            elems.push(format!("source: {}", self.source.unwrap()));
            elems.push(format!("type: {}", self.package_type.unwrap()));
            if !sys_deps.is_empty() {
                elems.push(format!("system deps: {sys_deps}"));
            }
            elems.join(", ")
        } else {
            let mut elems = Vec::new();
            elems.push(String::from("unresolved"));
            if let Some(e) = &self.error {
                elems.push(format!("error: {}", e));
            }
            if let Some(v) = &self.version_req {
                elems.push(format!("version requirement: {}", v));
            }
            elems.join(", ")
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
        if let Some(d) = max_depth {
            if current_depth > d {
                return;
            }
        }

        println!(
            "{prefix}{} {} [{}]",
            kind.prefix(),
            self.name,
            self.get_details(show_sys_deps)
        );

        let child_prefix = match kind {
            NodeKind::Normal => &format!("{prefix}│ "),
            NodeKind::Last => &format!("{prefix}  "),
        };

        for (idx, child) in self.children.iter().enumerate() {
            let child_kind = if idx == self.children.len() - 1 {
                NodeKind::Last
            } else {
                NodeKind::Normal
            };
            child.print_recursive(
                child_prefix,
                child_kind,
                current_depth + 1,
                max_depth,
                show_sys_deps,
            );
        }
    }
}

fn recursive_finder<'d>(
    name: &'d str,
    deps: Vec<&'d str>,
    deps_by_name: &HashMap<&'d str, &'d ResolvedDependency>,
    unresolved_deps_by_name: &HashMap<&'d str, &'d UnresolvedDependency>,
    context: &'d CliContext,
) -> TreeNode<'d> {
    if let Some(resolved) = deps_by_name.get(name) {
        let sys_deps = context.system_dependencies.get(name);
        let children: Vec<_> = deps
            .iter()
            .map(|x| {
                let resolved = deps_by_name[*x];
                recursive_finder(
                    x,
                    resolved.all_dependencies_names(),
                    deps_by_name,
                    unresolved_deps_by_name,
                    context,
                )
            })
            .collect();

        TreeNode {
            name,
            version: Some(resolved.version.as_ref()),
            source: Some(&resolved.source),
            package_type: Some(resolved.kind),
            resolved: true,
            error: None,
            version_req: None,
            sys_deps,
            children,
            ignored: resolved.ignored,
        }
    } else {
        let unresolved = unresolved_deps_by_name[name];
        TreeNode {
            name,
            version: None,
            source: None,
            package_type: None,
            sys_deps: None,
            error: unresolved.error.clone(),
            version_req: unresolved
                .version_requirement
                .clone()
                .map(|x| x.to_string()),
            resolved: false,
            children: vec![],
            ignored: false,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Tree<'a> {
    nodes: Vec<TreeNode<'a>>,
}

impl Tree<'_> {
    pub fn print(&self, max_depth: Option<usize>, show_sys_deps: bool) {
        for (i, tree) in self.nodes.iter().enumerate() {
            println!("▶ {} [{}]", tree.name, tree.get_details(show_sys_deps),);

            // Print children with standard indentation
            for (j, child) in tree.children.iter().enumerate() {
                let child_kind = if j == tree.children.len() - 1 {
                    NodeKind::Last
                } else {
                    NodeKind::Normal
                };
                child.print_recursive("", child_kind, 2, max_depth, show_sys_deps);
            }

            if i < self.nodes.len() - 1 {
                println!();
            }
        }
    }
}

/// Finds all paths from root nodes to the target package
fn find_all_paths_to_target<'a>(
    nodes: &[TreeNode<'a>], 
    target: &str, 
    current_path: Vec<&'a str>
) -> Vec<Vec<&'a str>> {
    let mut paths = Vec::new();
    
    for node in nodes {
        let mut new_path = current_path.clone();
        new_path.push(node.name);
        
        // If this node is our target, save this path
        if node.name == target {
            paths.push(new_path);
            // Don't recurse into children - we found our target
        } else {
            // Not our target, recurse into children
            let child_paths = find_all_paths_to_target(&node.children, target, new_path);
            paths.extend(child_paths);
        }
    }
    
    paths
}

/// Builds a set of all node names that should be kept based on the paths
fn build_nodes_to_keep(paths: &[Vec<&str>]) -> HashSet<String> {
    let mut nodes_to_keep = HashSet::new();
    
    for path in paths {
        for node_name in path {
            nodes_to_keep.insert(node_name.to_string());
        }
    }
    
    nodes_to_keep
}

/// Filters the tree to keep only nodes that are on paths to the target
fn filter_tree_by_marked_nodes<'a>(
    nodes: Vec<TreeNode<'a>>, 
    nodes_to_keep: &HashSet<String>,
    target: &str
) -> Vec<TreeNode<'a>> {
    nodes.into_iter()
        .filter_map(|mut node| {
            // Only keep nodes that are marked
            if nodes_to_keep.contains(node.name) {
                if node.name == target {
                    // If this is our target, clear its children - we don't care what it depends on
                    node.children = vec![];
                } else {
                    // Otherwise, recursively filter children
                    node.children = filter_tree_by_marked_nodes(
                        node.children, 
                        nodes_to_keep, 
                        target
                    );
                }
                Some(node)
            } else {
                None
            }
        })
        .collect()
}

/// Main filtering function that orchestrates the filtering process
fn filter_tree_for_package<'a>(nodes: Vec<TreeNode<'a>>, target: &str) -> Vec<TreeNode<'a>> {
    // Find all paths that lead to the target package
    let paths = find_all_paths_to_target(&nodes, target, Vec::new());
    
    // If no paths found, return empty tree
    if paths.is_empty() {
        return Vec::new();
    }
    
    // Build set of node names to keep
    let nodes_to_keep = build_nodes_to_keep(&paths);
    
    // Filter the tree
    filter_tree_by_marked_nodes(nodes, &nodes_to_keep, target)
}

pub fn tree<'a>(
    context: &'a CliContext,
    resolved_deps: &'a [ResolvedDependency],
    unresolved_deps: &'a [UnresolvedDependency],
    package_filter: Option<&str>,
) -> Tree<'a> {
    let deps_by_name: HashMap<_, _> = resolved_deps.iter().map(|d| (d.name.as_ref(), d)).collect();
    let unresolved_deps_by_name: HashMap<_, _> = unresolved_deps
        .iter()
        .map(|d| (d.name.as_ref(), d))
        .collect();

    let mut out = Vec::new();

    for top_level_dep in context.config.dependencies() {
        if let Some(found) = deps_by_name.get(top_level_dep.name()) {
            out.push(recursive_finder(
                found.name.as_ref(),
                found.all_dependencies_names(),
                &deps_by_name,
                &unresolved_deps_by_name,
                context,
            ));
        } else {
            let unresolved = unresolved_deps_by_name[top_level_dep.name()];
            out.push(TreeNode {
                name: top_level_dep.name(),
                version: None,
                source: None,
                package_type: None,
                sys_deps: None,
                error: unresolved.error.clone(),
                version_req: unresolved
                    .version_requirement
                    .clone()
                    .map(|x| x.to_string()),
                resolved: false,
                children: vec![],
                ignored: false,
            })
        }
    }

    // Apply package filtering if specified
    let filtered_nodes = if let Some(target_package) = package_filter {
        filter_tree_for_package(out, target_package)
    } else {
        out
    };

    Tree { nodes: filtered_nodes }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_node<'a>(name: &'a str, children: Vec<TreeNode<'a>>) -> TreeNode<'a> {
        TreeNode {
            name,
            version: None, // Simplified for testing
            source: None,
            package_type: None,
            sys_deps: None,
            resolved: true,
            error: None,
            version_req: None,
            children,
            ignored: false,
        }
    }

    #[test]
    fn test_filter_tree_for_package_simple() {
        // Create a simple tree: root -> target
        let target_node = create_test_node("target", vec![]);
        let root_node = create_test_node("root", vec![target_node]);
        let nodes = vec![root_node];

        let filtered = filter_tree_for_package(nodes, "target");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "root");
        assert_eq!(filtered[0].children.len(), 1);
        assert_eq!(filtered[0].children[0].name, "target");
        // Target should have no children (they're cleared)
        assert_eq!(filtered[0].children[0].children.len(), 0);
    }

    #[test]
    fn test_filter_tree_for_package_not_found() {
        // Create a tree without the target
        let child_node = create_test_node("child", vec![]);
        let root_node = create_test_node("root", vec![child_node]);
        let nodes = vec![root_node];

        let filtered = filter_tree_for_package(nodes, "nonexistent");

        // Should return empty tree
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_tree_for_package_multiple_paths() {
        // Create tree with multiple paths to target:
        // root1 -> target
        // root2 -> intermediate -> target
        let target1 = create_test_node("target", vec![]);
        let target2 = create_test_node("target", vec![]);
        let intermediate = create_test_node("intermediate", vec![target2]);
        
        let root1 = create_test_node("root1", vec![target1]);
        let root2 = create_test_node("root2", vec![intermediate]);
        let nodes = vec![root1, root2];

        let filtered = filter_tree_for_package(nodes, "target");

        assert_eq!(filtered.len(), 2);
        
        // Check first path: root1 -> target
        assert_eq!(filtered[0].name, "root1");
        assert_eq!(filtered[0].children.len(), 1);
        assert_eq!(filtered[0].children[0].name, "target");
        
        // Check second path: root2 -> intermediate -> target
        assert_eq!(filtered[1].name, "root2");
        assert_eq!(filtered[1].children.len(), 1);
        assert_eq!(filtered[1].children[0].name, "intermediate");
        assert_eq!(filtered[1].children[0].children.len(), 1);
        assert_eq!(filtered[1].children[0].children[0].name, "target");
    }

    #[test]
    fn test_filter_tree_removes_irrelevant_branches() {
        // Create tree:
        // root -> target
        //      -> irrelevant -> more_irrelevant
        let target = create_test_node("target", vec![]);
        let more_irrelevant = create_test_node("more_irrelevant", vec![]);
        let irrelevant = create_test_node("irrelevant", vec![more_irrelevant]);
        let root = create_test_node("root", vec![target, irrelevant]);
        let nodes = vec![root];

        let filtered = filter_tree_for_package(nodes, "target");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "root");
        // Should only have the target child, not the irrelevant branch
        assert_eq!(filtered[0].children.len(), 1);
        assert_eq!(filtered[0].children[0].name, "target");
    }

    #[test]
    fn test_filter_clears_target_children() {
        // Create: target -> child1 -> child2
        // When filtering for "target", its children should be cleared
        let child2 = create_test_node("child2", vec![]);
        let child1 = create_test_node("child1", vec![child2]);
        let target = create_test_node("target", vec![child1]);
        let nodes = vec![target];

        let filtered = filter_tree_for_package(nodes, "target");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "target");
        // Children should be cleared since this is the target
        assert_eq!(filtered[0].children.len(), 0);
    }

    #[test]
    fn test_find_all_paths_to_target() {
        // Create tree with multiple paths:
        // root1 -> target
        // root2 -> intermediate -> target  
        let target1 = create_test_node("target", vec![]);
        let target2 = create_test_node("target", vec![]);
        let intermediate = create_test_node("intermediate", vec![target2]);
        let root1 = create_test_node("root1", vec![target1]);
        let root2 = create_test_node("root2", vec![intermediate]);
        let nodes = vec![root1, root2];

        let paths = find_all_paths_to_target(&nodes, "target", Vec::new());

        assert_eq!(paths.len(), 2);
        
        // Should find both paths
        assert!(paths.contains(&vec!["root1", "target"]));
        assert!(paths.contains(&vec!["root2", "intermediate", "target"]));
    }

    #[test]
    fn test_filter_target_with_and_without_children() {
        // Create tree where target appears both as intermediate and leaf:
        // root -> target -> child
        //      -> other -> target (leaf)
        let target_leaf = create_test_node("target", vec![]);
        let child = create_test_node("child", vec![]);
        let target_intermediate = create_test_node("target", vec![child]);
        let other = create_test_node("other", vec![target_leaf]);
        let root = create_test_node("root", vec![target_intermediate, other]);
        let nodes = vec![root];

        let filtered = filter_tree_for_package(nodes, "target");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "root");
        assert_eq!(filtered[0].children.len(), 2);
        
        // Both target nodes should have their children cleared
        let target_children: Vec<_> = filtered[0].children.iter()
            .filter(|child| child.name == "target")
            .collect();
        assert_eq!(target_children.len(), 1);
        assert_eq!(target_children[0].children.len(), 0); // Children cleared
        
        // The "other" intermediate node should be kept
        let other_children: Vec<_> = filtered[0].children.iter()
            .filter(|child| child.name == "other")
            .collect();
        assert_eq!(other_children.len(), 1);
        assert_eq!(other_children[0].children.len(), 1); // Contains target
        assert_eq!(other_children[0].children[0].name, "target");
        assert_eq!(other_children[0].children[0].children.len(), 0); // Target's children cleared
    }
}
