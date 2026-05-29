use std::{borrow::Cow, collections::VecDeque, error::Error};

use crate::{ResolvedDependency, UnresolvedDependency, package::NeedsEntry, resolver::QueueItem};

pub(crate) fn extend_queue_with_needs<'d>(
    item: &QueueItem<'d>,
    resolved_dep: &ResolvedDependency<'d>,
    queue: &mut VecDeque<QueueItem<'d>>,
    failed: &mut Vec<UnresolvedDependency<'d>>,
) {
    match build_needs_queue_items(item, resolved_dep) {
        Ok(items) => queue.extend(items),
        Err(e) => failed.push(UnresolvedDependency::from_item(item).with_error(e.to_string())),
    }
}

/// Generates queue items for Config/Needs/* entries from a resolved dependency.
fn build_needs_queue_items<'d>(
    item: &QueueItem<'d>,
    resolved_dep: &ResolvedDependency<'d>,
) -> Result<Vec<QueueItem<'d>>, Box<dyn Error>> {
    if !item.install_all_needs && item.needs.is_empty() {
        return Ok(vec![]);
    }
    if resolved_dep.needs.is_empty() {
        log::debug!(
            "No Config/Needs/* data available for {} (likely from lockfile without fresh DESCRIPTION fetch)",
            resolved_dep.name
        );
        return Ok(vec![]);
    }

    let entries: Vec<&NeedsEntry> = if item.install_all_needs {
        resolved_dep.needs.values().flatten().collect()
    } else {
        let mut declared_needs = item.needs.clone();
        declared_needs.retain(|need| !resolved_dep.needs.contains_key(need));
        if !declared_needs.is_empty() {
            return Err(format!(
                "{} declares need(s) `[{}]` which are not found in the package",
                item.name,
                declared_needs.join(", ")
            )
            .into());
        }

        item.needs
            .iter()
            .filter_map(|k| resolved_dep.needs.get(k))
            .flatten()
            .collect()
    };

    let items = entries
        .into_iter()
        .map(|entry| match entry {
            NeedsEntry::Package(dep) => {
                let mut i = QueueItem::name_and_parent_only(
                    Cow::Owned(dep.name().to_string()),
                    resolved_dep.name.clone(),
                );
                i.version_requirement = dep.version_requirement().map(|r| Cow::Owned(r.clone()));
                i
            }
            NeedsEntry::Remote(pkg_name, remote) => {
                let mut i = QueueItem::name_and_parent_only(
                    Cow::Owned(pkg_name.clone()),
                    resolved_dep.name.clone(),
                );
                i.remote = Some(remote.clone());
                i.from_needs_remote = true;
                i
            }
        })
        .collect();
    Ok(items)
}
