use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::Result;
use fs_err as fs;

use rv::cli::utils::{timeit, write_err};
use rv::cli::{migrate, sync, CliContext};
use rv::{ResolvedDependency, Resolver};

#[derive(Parser)]
#[clap(version, author, about, subcommand_negates_reqs = true)]
pub struct Cli {
    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,

    /// Path to a config file other than rproject.toml in the current directory
    #[clap(short = 'c', long, default_value = "rproject.toml")]
    pub config_file: PathBuf,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Creates a new rv project
    Init,
    /// Dry run of what sync would do
    Plan,
    /// Replaces the library with exactly what is in the lock file
    Sync,
    /// Migrate other package management formats
    Migrate,
}

/// Resolve dependencies for the project. If there are any unmet dependencies, they will be printed
/// to stderr and the cli will exit.
fn resolve_dependencies(context: &CliContext) -> Vec<ResolvedDependency> {
    let resolver = Resolver::new(&context.databases, &context.r_version);
    let (resolved, unresolved) = resolver.resolve(context.config.dependencies(), &context.cache);
    if !unresolved.is_empty() {
        eprintln!("Failed to resolve all dependencies");
        for d in unresolved {
            eprintln!("    {d}");
        }
        ::std::process::exit(1)
    }

    resolved
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
        Command::Plan => {
            let context = CliContext::new(&cli.config_file)?;
            let resolved = resolve_dependencies(&context);

            println!("Plan successful! The following packages will be installed:");
            for d in resolved {
                println!("    {d}");
            }
        }
        Command::Sync => {
            let context = CliContext::new(&cli.config_file)?;
            let resolved = resolve_dependencies(&context);
            match timeit!("Synced dependencies", sync(&context, resolved)) {
                Ok(changes) => {
                    for c in changes {
                        if c.installed {
                            println!(
                                "+ {} ({}) in {}ms",
                                c.name,
                                c.version.unwrap(),
                                c.timing.unwrap().as_millis()
                            );
                        } else {
                            println!("- {}", c.name);
                        }
                    }
                }
                Err(e) => {
                    if context.staging_path().is_dir() {
                        fs::remove_dir_all(context.staging_path())?;
                    }
                    return Err(e);
                }
            }
        }
        Command::Migrate => {
            let context = CliContext::new(&cli.config_file)?;
            migrate(context);
        }
    }

    Ok(())
}

fn main() {
    if let Err(e) = try_main() {
        eprintln!("{}", write_err(&*e));
        ::std::process::exit(1)
    }
}
