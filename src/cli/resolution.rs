use crate::{Context, Resolution, ResolveMode};

/// Resolve dependencies for the project. If there are any unmet dependencies, they will be printed
/// to stderr and the cli may exit.
pub fn resolve_dependencies(
    context: &Context,
    resolve_mode: ResolveMode,
    exit_on_failure: bool,
) -> Resolution<'_> {
    let resolution = context.resolve(resolve_mode);

    if !resolution.is_success() && exit_on_failure {
        resolution.print_failures();
        ::std::process::exit(1)
    }

    resolution
}
