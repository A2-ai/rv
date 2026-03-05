use crate::Context;
use crate::lockfile::Source;
use crate::package::PackageType;
use crate::{ResolvedDependency, UnresolvedDependency, Version};
use serde::Serialize;
use std::collections::HashMap;

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
        }
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
        }
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
                )
            })
            .collect();
        ancestors.pop();

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
            println!("▶ {} [{}]", tree.name, tree.get_details(show_sys_deps),);

            for (j, child) in tree.children.iter().enumerate() {
                child.print_recursive(
                    "",
                    child_kind(j, tree.children.len()),
                    2,
                    max_depth,
                    show_sys_deps,
                );
            }

            if i + 1 < self.nodes.len() {
                println!();
            }
        }
    }
}

pub fn tree<'a>(
    context: &'a Context,
    resolved_deps: &'a [ResolvedDependency],
    unresolved_deps: &'a [UnresolvedDependency],
) -> Tree<'a> {
    let deps_by_name: HashMap<_, _> = resolved_deps.iter().map(|d| (d.name.as_ref(), d)).collect();
    let unresolved_deps_by_name: HashMap<_, _> = unresolved_deps
        .iter()
        .map(|d| (d.name.as_ref(), d))
        .collect();

    let mut nodes = Vec::new();

    for top_level_dep in context.config.dependencies() {
        if let Some(found) = deps_by_name.get(top_level_dep.name()) {
            let mut ancestors = Vec::new();
            nodes.push(recursive_finder(
                found.name.as_ref(),
                &deps_by_name,
                &unresolved_deps_by_name,
                context,
                &mut ancestors,
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
