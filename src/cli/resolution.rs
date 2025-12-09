use crate::{Context, Resolution, ResolveMode};

/// Resolve dependencies for the project. If there are any unmet dependencies, they will be printed
/// to stderr and the cli will exit.
pub fn resolve_dependencies<'a>(
    context: &'a Context,
    resolve_mode: &ResolveMode,
    exit_on_failure: bool,
) -> Resolution<'a> {
    let resolution = context.resolve(resolve_mode);

    if !resolution.is_success() && exit_on_failure {
        eprintln!("Failed to resolve all dependencies");
        let req_error_messages = resolution.req_error_messages();

        for d in &resolution.failed {
            eprintln!("    {d}");
        }

        if !req_error_messages.is_empty() {
            eprintln!("{}", req_error_messages.join("\n"));
        }

        ::std::process::exit(1)
    }

    resolution
}
