use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::Result;
use fs_err as fs;
use rv::cli::utils::timeit;
use rv::cli::{find_r_repositories, init, migrate_renv, sync, CacheInfo, CliContext};
use rv::{
    add_packages, Git, Http, Lockfile, RCmd, RCommandLine, ResolvedDependency, Resolver
};

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
    Init {
        #[clap(value_parser)]
        project_directory: PathBuf,
    },
    /// Returns the path for the library for the current project/system
    Library,
    /// Dry run of what sync would do
    Plan,
    /// Replaces the library with exactly what is in the lock file
    Sync,
    /// Add to a simple package to the project and sync
    Add {
        #[clap(value_parser)]
        packages: Vec<String>,
        #[clap(long)]
        plan: bool,
        #[clap(long)]
        no_sync: bool,
    },
    /// Gives information about where the cache is for that project
    Cache {
        #[clap(short, long)]
        json: bool,
    },
    /// Migrate renv to rv
    Migrate {
        #[clap(subcommand)]
        subcommand: MigrateSubcommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum AddSubcommand {
    Pkg {
        #[clap(value_parser, default_value = "renv.lock")]
        packages: Vec<String>,
        #[clap(long)]
        plan: bool,
        #[clap(long)]
        sync: bool,
    }
}
   

#[derive(Debug, Subcommand)]
pub enum MigrateSubcommand {
    Renv {
        #[clap(value_parser, default_value = "renv.lock")]
        renv_file: PathBuf,
    },
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
                if resolved.is_empty() {
                    // delete the lockfiles if there are no dependencies
                    let lockfile_path = context.lockfile_path();
                    if lockfile_path.exists() {
                        fs::remove_file(lockfile_path)?;
                    }
                } else {
                    let lockfile =
                        Lockfile::from_resolved(&context.r_version.major_minor(), resolved);
                    if let Some(existing_lockfile) = &context.lockfile {
                        if existing_lockfile != &lockfile {
                            lockfile.save(context.lockfile_path())?;
                            log::debug!("Lockfile changed, saving it.");
                        }
                    } else {
                        lockfile.save(context.lockfile_path())?;
                    }
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
        .filter(Some("os_info"), log::LevelFilter::Off)
        .init();

    match cli.command {
        Command::Init { project_directory } => {
            if project_directory.exists() {
                println!("{} already exists", project_directory.display());
                return Ok(());
            }
            // TODO: use cli flag for non-default r_version
            let r_version = RCommandLine { r: None }.version()?;
            // TODO: use cli flag to turn off default repositories (or specify non-default repos)
            let repositories = find_r_repositories()?;
            init(&project_directory, &r_version.major_minor(), &repositories)?;
            println!(
                "rv project successfully initialized at {}",
                project_directory.display()
            );
        }
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
        Command::Add{packages, plan, no_sync} => {
            add_packages(packages, &cli.config_file)?;
            if plan {
                _sync(&cli.config_file, true)?;
            } else if !no_sync {
                _sync(&cli.config_file, false)?;
            }
        }
        Command::Cache { json } => {
            let context = CliContext::new(&cli.config_file)?;
            let info = CacheInfo::new(&context, resolve_dependencies(&context));
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&info).expect("valid json")
                );
            } else {
                println!("{info}");
            }
        }
        Command::Migrate {
            subcommand: MigrateSubcommand::Renv { renv_file },
        } => {
            let unresolved = migrate_renv(&renv_file, &cli.config_file)?;
            if unresolved.is_empty() {
                println!(
                    "{} was successfully migrated to {}",
                    renv_file.display(),
                    cli.config_file.display()
                );
            } else {
                println!(
                    "{} was migrated to {} with {} unresolved packages: ",
                    renv_file.display(),
                    cli.config_file.display(),
                    unresolved.len()
                );
                for u in &unresolved {
                    eprintln!("    {u}");
                }
            }
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
