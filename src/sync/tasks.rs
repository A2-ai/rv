//! The sync task tree for progress events, defined together so the labels and parent
//! relationships are visible in one place. Each child composes off its parent via
//! [`events::Task::child`], so the `sync:{name}` id scheme lives in exactly one spot.

use crate::events;

pub(crate) fn sync_task() -> events::Task {
    events::Task::new("sync", "Syncing")
}

pub(crate) fn install_task(name: &str) -> events::Task {
    sync_task().child(name, name)
}

/// Cloning a git dependency, nested under that package's install task.
pub(crate) fn clone_task(name: &str) -> events::Task {
    install_task(name).child("clone", "Cloning git repository")
}

/// Compiling a source package, nested under that package's install task.
pub(crate) fn compile_task(name: &str) -> events::Task {
    install_task(name).child("compile", "Compiling")
}
