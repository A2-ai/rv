use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::Result;
use fs_err as fs;

use rv::cli::utils::timeit;
use rv::cli::{sync, CliContext};
use rv::{Git, Http, Lockfile, ResolvedDependency, Resolver};

#[derive(Parser)]
#[clap(version, author, about, subcommand_negates_reqs = true)]
pub struct Cli {
    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,

    /// Path to a config file other than rproject.toml in the current directory
    #[clap(short = 'c', long, default_value = "rproject.toml", global = true)]
    pub config_file: PathBuf,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Creates a new rv project
    Init,
    /// Returns the path for the library for the current project/system
    Library,
    /// Dry run of what sync would do
    Plan,
    /// Replaces the library with exactly what is in the lock file
    Sync,
}

/// Resolve dependencies for the project. If there are any unmet dependencies, they will be printed
/// to stderr and the cli will exit.
fn resolve_dependencies(context: &CliContext) -> Vec<ResolvedDependency> {
    let resolver = Resolver::new(
        &context.databases,
        &context.r_version,
        context.lockfile.as_ref(),
    );

    let resolution = resolver.resolve(
        context.config.dependencies(),
        context.config.prefer_repositories_for(),
        &context.cache,
        &Git {},
        &Http {},
    );
    if !resolution.is_success() {
        eprintln!("Failed to resolve all dependencies");
        for d in resolution.failed {
            eprintln!("    {d}");
        }
        ::std::process::exit(1)
    }

    resolution.found
}

fn _sync(config_file: &PathBuf, dry_run: bool) -> Result<()> {
    let mut context = CliContext::new(config_file)?;
    context.load_databases_if_needed()?;
    let resolved = resolve_dependencies(&context);

    match timeit!(
        if dry_run {
            "Planned dependencies"
        } else {
            "Synced dependencies"
        },
        sync(&context, &resolved, &context.library, dry_run)
    ) {
        Ok(changes) => {
            if changes.is_empty() {
                println!("Nothing to do");
            }
            if !dry_run {
                let lockfile = Lockfile::from_resolved(&context.r_version.major_minor(), resolved);
                if let Some(existing_lockfile) = &context.lockfile {
                    if existing_lockfile != &lockfile {
                        lockfile.save(context.lockfile_path())?;
                        log::debug!("Lockfile changed, saving it.");
                    }
                } else {
                    lockfile.save(context.lockfile_path())?;
                }
            }

            for c in changes {
                println!("{}", c.print(!dry_run));
            }
            Ok(())
        }
        Err(e) => {
            if context.staging_path().is_dir() {
                fs::remove_dir_all(context.staging_path())?;
            }
            Err(e)
        }
    }
}

fn try_main() -> Result<()> {
    let cli = Cli::parse();
    env_logger::Builder::new()
        .filter_level(cli.verbose.log_level_filter())
        .filter(Some("ureq"), log::LevelFilter::Off)
        .filter(Some("rustls"), log::LevelFilter::Off)
        .init();

    match cli.command {
        Command::Init => todo!("implement init"),
        Command::Library => {
            let context = CliContext::new(&cli.config_file)?;
            println!("{}", context.library_path().display());
        }
        Command::Plan => {
            _sync(&cli.config_file, true)?;
        }
        Command::Sync => {
            _sync(&cli.config_file, false)?;
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = try_main() {
        eprintln!("{e:?}");
        ::std::process::exit(1)
    }
}
