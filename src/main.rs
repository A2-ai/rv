use clap::{Parser, Subcommand};
use std::path::PathBuf;

use anyhow::Result;
use fs_err::{self as fs, read_to_string, write};
use rv::cli::utils::timeit;
use rv::cli::{CliContext, find_r_repositories, init, init_structure, migrate_renv};
use rv::{
    CacheInfo, Config, GitExecutor, Http, Lockfile, ProjectSummary, RCmd, RCommandLine,
    ResolvedDependency, Resolver, SyncHandler, Version, activate, add_packages, deactivate,
    read_and_verify_config,
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
        #[clap(value_parser, default_value = ".")]
        project_directory: PathBuf,
        #[clap(short = 'r', long)]
        /// Specify a non-default R version
        r_version: Option<Version>,
        #[clap(long)]
        /// Do no populated repositories
        no_repositories: bool,
        #[clap(long, value_parser, num_args = 1..)]
        /// Add simple packages to the config
        add: Vec<String>,
        #[clap(long)]
        /// Turn off rv access through .rv R environment
        no_r_environment: bool,
        #[clap(long)]
        /// Force new init. This will replace content in your rproject.toml
        force: bool,
    },
    /// Returns the path for the library for the current project/system.
    /// The path is always in unix format
    Library,
    /// Dry run of what sync would do
    Plan {
        #[clap(short, long)]
        upgrade: bool,
        /// Specify a R version different from the one in the config.
        /// The command will not error even if this R version is not found
        #[clap(long)]
        r_version: Option<Version>,
    },
    /// Replaces the library with exactly what is in the lock file
    Sync,
    /// Add simple packages to the project and sync
    Add {
        #[clap(value_parser)]
        packages: Vec<String>,
        #[clap(long)]
        /// Do not make any changes, only report what would happen if those packages were added         
        dry_run: bool,
        #[clap(long)]
        /// Add packages to config file, but do not sync. No effect if --dry-run is used
        no_sync: bool,
    },
    /// Provide a summary about the project status
    Summary {
        #[clap(short, long)]
        json: bool,
    },
    /// Simple information about the project
    Info {
        #[clap(long)]
        /// The relative library path
        library: bool,
        #[clap(long)]
        /// The R version specified in the config
        r_version: bool,
        #[clap(long)]
        /// The repositories specified in the config
        #[clap(long)]
        repositories: bool,
    },
    /// Gives information about where the cache is for that project
    Cache {
        #[clap(short, long)]
        json: bool,
    },
    /// Upgrade packages to the latest versions available
    Upgrade {
        #[clap(long)]
        dry_run: bool,
    },
    /// Migrate renv to rv
    Migrate {
        #[clap(subcommand)]
        subcommand: MigrateSubcommand,
    },
    /// Activate a previously initialized rv project
    Activate {
        #[clap(long)]
        no_r_environment: bool,
    },
    /// Deactivate an rv project
    Deactivate,
}

#[derive(Debug, Subcommand)]
pub enum MigrateSubcommand {
    Renv {
        #[clap(value_parser, default_value = "renv.lock")]
        renv_file: PathBuf,
        #[clap(long)]
        /// Include the patch in the R version
        strict_r_version: bool,
        /// Turn off rv access through .rv R environment
        no_r_environment: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum ResolveMode {
    Default,
    FullUpgrade,
    // TODO: PartialUpgrade -- allow user to specify packages to upgrade
}

/// Resolve dependencies for the project. If there are any unmet dependencies, they will be printed
/// to stderr and the cli will exit.
fn resolve_dependencies<'a>(
    context: &'a CliContext,
    resolve_mode: &ResolveMode,
) -> Vec<ResolvedDependency<'a>> {
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
    );

    if context.show_progress_bar {
        resolver.show_progress_bar();
    }

    let resolution = resolver.resolve(
        context.config.dependencies(),
        context.config.prefer_repositories_for(),
        &context.cache,
        &GitExecutor {},
        &Http {},
    );
    if !resolution.is_success() {
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
    let resolved = if resolve_mode == &ResolveMode::FullUpgrade && context.lockfile.is_some() {
        resolution
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
            .collect::<Vec<_>>()
    } else {
        resolution.found
    };

    resolved
}

fn _sync(
    mut context: CliContext,
    dry_run: bool,
    has_logs_enabled: bool,
    resolve_mode: ResolveMode,
) -> Result<()> {
    if !has_logs_enabled {
        context.show_progress_bar();
    }

    // If the sync mode is an upgrade, we want to load the databases even if all packages are contained in the lockfile
    // because we ignore the lockfile during initial resolution
    match resolve_mode {
        ResolveMode::Default => context.load_databases_if_needed()?,
        ResolveMode::FullUpgrade => context.load_databases()?,
    }

    let resolved = resolve_dependencies(&context, &resolve_mode);

    match timeit!(
        if dry_run {
            "Planned dependencies"
        } else {
            "Synced dependencies"
        },
        {
            let mut handler = SyncHandler::new(
                &context.project_dir,
                &context.library,
                &context.cache,
                &context.staging_path(),
            );
            if dry_run {
                handler.dry_run();
            }
            if !has_logs_enabled {
                handler.show_progress_bar();
            }
            handler.set_has_lockfile(context.lockfile.is_some());
            handler.handle(&resolved, &context.r_cmd)
        }
    ) {
        Ok(changes) => {
            if changes.is_empty() {
                println!("Nothing to do");
            }
            if !dry_run && context.config.use_lockfile() {
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
            Err(e.into())
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
        Command::Init {
            project_directory,
            r_version,
            no_repositories,
            add,
            no_r_environment,
            force,
        } => {
            let r_version = if let Some(r) = r_version {
                r.original
            } else {
                // if r version is not provided, get the major.minor of the R version on the path
                let [major, minor] = match (RCommandLine { r: None }).version() {
                    Ok(r_ver) => r_ver,
                    Err(e) => {
                        if cfg!(windows) {
                            RCommandLine {
                                r: Some(PathBuf::from("R.bat")),
                            }
                            .version()?
                        } else {
                            Err(e)?
                        }
                    }
                }
                .major_minor();
                format!("{major}.{minor}")
            };

            let repositories = if no_repositories {
                Vec::new()
            } else {
                find_r_repositories().unwrap_or(Vec::new())
            };
            init(&project_directory, &r_version, &repositories, &add, force)?;
            activate(&project_directory, no_r_environment)?;
            println!(
                "rv project successfully initialized at {}",
                project_directory.display()
            );
        }
        Command::Library => {
            let context = CliContext::new(&cli.config_file, None)?;
            let path_str = context.library_path().to_string_lossy();
            let path_out = if cfg!(windows) {
                path_str.replace('\\', "/")
            } else {
                path_str.to_string()
            };
            println!("{path_out}");
        }
        Command::Plan { upgrade, r_version } => {
            let upgrade = if upgrade || r_version.is_some() {
                ResolveMode::FullUpgrade
            } else {
                ResolveMode::Default
            };
            let context = CliContext::new(&cli.config_file, r_version)?;
            _sync(context, true, cli.verbose.is_present(), upgrade)?;
        }
        Command::Sync => {
            let context = CliContext::new(&cli.config_file, None)?;
            _sync(
                context,
                false,
                cli.verbose.is_present(),
                ResolveMode::Default,
            )?;
        }
        Command::Add {
            packages,
            dry_run,
            no_sync,
        } => {
            // load config to verify structure is valid
            let mut doc = read_and_verify_config(&cli.config_file)?;
            add_packages(&mut doc, packages)?;
            // write the update if not dry run
            if !dry_run {
                write(&cli.config_file, doc.to_string())?;
            }
            // if no sync, exit early
            if no_sync {
                println!("Packages successfully added");
                return Ok(());
            }
            let mut context = CliContext::new(&cli.config_file, None)?;
            // if dry run, the config won't have been edited to reflect the added changes so must be added
            if dry_run {
                context.config = doc.to_string().parse::<Config>()?;
            }
            _sync(
                context,
                dry_run,
                cli.verbose.is_present(),
                ResolveMode::Default,
            )?;
        }
        Command::Upgrade { dry_run } => {
            let context = CliContext::new(&cli.config_file, None)?;
            _sync(
                context,
                dry_run,
                cli.verbose.is_present(),
                ResolveMode::FullUpgrade,
            )?;
        }
        Command::Info {
            library,
            r_version,
            repositories,
        } => {
            let context = CliContext::new(&cli.config_file, None)?;
            if library {
                let path_str = context.library_path().to_string_lossy();
                let path_out = if cfg!(windows) {
                    path_str.replace('\\', "/")
                } else {
                    path_str.to_string()
                };
                println!("library: {path_out}");
            }
            if r_version {
                println!("r-version: {}", context.r_version.original);
            }
            if repositories {
                let repos = context
                    .config
                    .repositories()
                    .iter()
                    .map(|r| format!("({}, {})", r.alias, r.url()))
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("repositories: {}", repos);
            }
        }
        Command::Cache { json } => {
            let mut context = CliContext::new(&cli.config_file, None)?;
            context.load_databases()?;
            let info = CacheInfo::new(
                &context.config,
                &context.cache,
                resolve_dependencies(&context, &ResolveMode::Default),
            );
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
            subcommand:
                MigrateSubcommand::Renv {
                    renv_file,
                    strict_r_version,
                    no_r_environment,
                },
        } => {
            let unresolved = migrate_renv(&renv_file, &cli.config_file, strict_r_version)?;
            // migrate renv will create the config file, so parent directory is confirmed to exist
            let project_dir = &cli
                .config_file
                .canonicalize()?
                .parent()
                .unwrap()
                .to_path_buf();
            init_structure(project_dir)?;
            activate(project_dir, no_r_environment)?;
            let content = read_to_string(project_dir.join(".Rprofile"))?.replace(
                "source(\"renv/activate.R\")",
                "# source(\"renv/activate.R\")",
            );
            write(project_dir.join(".Rprofile"), content)?;
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
        Command::Summary { json } => {
            let mut context = CliContext::new(&cli.config_file, None)?;
            context.load_databases()?;
            let resolved = resolve_dependencies(&context, &ResolveMode::Default);
            let summary = ProjectSummary::new(
                &context.library,
                &resolved,
                &context.config.repositories(),
                &context.databases,
                &context.r_version,
                &context.cache,
                context.lockfile.as_ref(),
            );
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).expect("valid json")
                );
            } else {
                println!("{summary}");
            }
        }
        Command::Activate { no_r_environment } => {
            let dir = std::env::current_dir()?;
            activate(dir, no_r_environment)?;
            println!("rv activated");
        }
        Command::Deactivate => {
            let dir = std::env::current_dir()?;
            deactivate(dir)?;
            println!("rv deactivated");
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
