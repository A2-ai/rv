use crate::cli::CliContext;
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

pub fn tree<'a>(
    context: &'a CliContext,
    resolved_deps: &'a [ResolvedDependency],
    unresolved_deps: &'a [UnresolvedDependency],
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
            })
        }
    }

    Tree { nodes: out }
}
