use crate::{Context, Resolution, ResolveMode, Resolver};
use crate::{GitExecutor, Http};

/// Resolve dependencies for the project. If there are any unmet dependencies, they will be printed
/// to stderr and the cli will exit.
pub fn resolve_dependencies<'a>(
    context: &'a Context,
    resolve_mode: &ResolveMode,
    exit_on_failure: bool,
) -> Resolution<'a> {
    let lockfile = match resolve_mode {
        ResolveMode::Default => &context.lockfile,
        ResolveMode::FullUpgrade => &None,
    };

    let mut resolver = Resolver::new(
        &context.project_dir,
        &context.databases,
        context
            .config
            .repositories()
            .iter()
            .map(|x| x.url())
            .collect(),
        &context.r_version,
        &context.builtin_packages,
        lockfile.as_ref(),
        context.config.packages_env_vars(),
    );

    if context.show_progress_bar {
        resolver.show_progress_bar();
    }

    let mut resolution = resolver.resolve(
        context.config.dependencies(),
        context.config.prefer_repositories_for(),
        &context.cache,
        &GitExecutor {},
        &Http {},
    );

    if !resolution.is_success() && exit_on_failure {
        eprintln!("Failed to resolve all dependencies");
        let req_error_messages = resolution.req_error_messages();

        for d in resolution.failed {
            eprintln!("    {d}");
        }

        if !req_error_messages.is_empty() {
            eprintln!("{}", req_error_messages.join("\n"));
        }

        ::std::process::exit(1)
    }

    // If upgrade and there is a lockfile, we want to adjust the resolved dependencies s.t. if the resolved dep has the same
    // name and version in the lockfile, we say that it was resolved from the lockfile
    if resolve_mode == &ResolveMode::FullUpgrade && context.lockfile.is_some() {
        resolution.found = resolution
            .found
            .into_iter()
            .map(|mut dep| {
                dep.from_lockfile = context
                    .lockfile
                    .as_ref()
                    .unwrap()
                    .contains_resolved_dep(&dep);
                dep
            })
            .collect::<Vec<_>>();
    }

    resolution
}
