use clap::{Parser, Subcommand};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use url::Url;

use anyhow::{Result, anyhow};
use fs_err::{self as fs, read_to_string, write};
use serde::Serialize;
use serde_json::json;

use rv::cli::utils::timeit;
use rv::cli::{
    CliContext, RCommandLookup, find_r_repositories, init, init_structure, migrate_renv, tree,
};
use rv::system_req::{SysDep, SysInstallationStatus};
use rv::{
    CacheInfo, Config, ConfigureRepositoryResponse, GitExecutor, Http, Lockfile, ProjectSummary,
    RCmd, RCommandLine, RepositoryAction, RepositoryMatcher, RepositoryPositioning,
    RepositoryUpdates, Resolution, Resolver, SyncChange, SyncHandler, Version, activate,
    add_packages, deactivate, execute_repository_action, read_and_verify_config, system_req,
};
use rv::{
    DependencyAction, DependencyType, GitDepRef, RepositoryOperation as LibRepositoryOperation,
};

#[derive(Parser)]
#[clap(version, author, about, subcommand_negates_reqs = true)]
pub struct Cli {
    #[command(flatten)]
    verbose: clap_verbosity_flag::Verbosity,

    /// Output in JSON format. This will also ignore the --verbose flag and not log anything.
    #[clap(long, global = true)]
    json: bool,

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
    /// Returns the path for the library for the current project/system in UNIX format, even
    /// on Windows.
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
    Sync {
        #[clap(long)]
        save_install_logs_in: Option<PathBuf>,
    },
    /// Add simple packages to the project and sync
    Add {
        #[clap(value_parser, required = true)]
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
        /// Specify a R version different from the one in the config.
        /// The command will not error even if this R version is not found
        #[clap(long)]
        r_version: Option<Version>,
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
    Cache,
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
    /// List the system dependencies needed by the dependency tree.
    /// This is currently only supported on Ubuntu/Debian, it will return an empty result
    /// anywhere else.
    ///
    /// The present/absent status may be wrong if a dependency was installed in
    /// a way that we couldn't detect (eg not via the main package manager of the OS).
    /// If a dependency that you know is installed but is showing up as
    Sysdeps {
        /// Only show the dependencies not detected on the system.
        #[clap(long)]
        only_absent: bool,

        /// Ignore the dependencies in that list from the output.
        /// For example if you have installed pandoc manually without using the OS package manager
        /// and want to not return it from this command.
        #[clap(long)]
        ignore: Vec<String>,
    },
    /// Shows the project packages in tree format
    Tree {
        #[clap(long)]
        /// How deep are we going in the tree: 1 == only root deps, 2 == root deps + their direct dep etc
        /// Defaults to showing everything
        depth: Option<usize>,
        #[clap(long)]
        /// Whether to not display the system dependencies on each leaf.
        /// This only does anything on supported platforms (eg some Linux), it's already
        /// hidden otherwise
        hide_system_deps: bool,
        #[clap(long)]
        /// Specify a R version different from the one in the config.
        /// The command will not error even if this R version is not found
        r_version: Option<Version>,
    },
    /// Configure project settings
    Configure {
        #[command(subcommand)]
        subcommand: ConfigureSubcommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigureSubcommand {
    /// Configure project repositories
    Repository {
        #[clap(subcommand)]
        operation: RepositoryOperation,
    },
    Dependency {
        #[clap(subcommand)]
        operation: DependencyOperation,
    },
}

#[derive(Debug, Subcommand)]
pub enum RepositoryOperation {
    /// Add a new repository
    Add {
        /// Repository alias
        alias: String,
        /// Repository URL
        #[clap(long)]
        url: String,
        /// Enable force_source for this repository
        #[clap(long)]
        force_source: bool,
        /// Add as first repository
        #[clap(long, conflicts_with_all = ["last", "before", "after"])]
        first: bool,
        /// Add as last repository (default)
        #[clap(long, conflicts_with_all = ["first", "before", "after"])]
        last: bool,
        /// Add before the specified alias
        #[clap(long, conflicts_with_all = ["first", "last", "after"])]
        before: Option<String>,
        /// Add after the specified alias
        #[clap(long, conflicts_with_all = ["first", "last", "before"])]
        after: Option<String>,
    },
    /// Replace an existing repository (keeps original alias if not specified)
    Replace {
        /// Repository alias to replace
        old_alias: String,
        /// New repository alias (optional, keeps original if not specified)
        #[clap(long)]
        alias: Option<String>,
        /// Repository URL
        #[clap(long)]
        url: String,
        /// Enable/disable force_source for this repository
        #[clap(long)]
        force_source: bool,
    },
    /// Update an existing repository (partial updates)
    Update {
        /// Repository alias to update (if not using --match-url)
        target_alias: Option<String>,
        /// Match repository by URL instead of alias
        #[clap(long, conflicts_with = "target_alias")]
        match_url: Option<String>,
        /// New repository alias
        #[clap(long)]
        alias: Option<String>,
        /// New repository URL
        #[clap(long)]
        url: Option<String>,
        /// Enable force_source
        #[clap(long, conflicts_with = "no_force_source")]
        force_source: bool,
        /// Disable force_source
        #[clap(long, conflicts_with = "force_source")]
        no_force_source: bool,
    },
    /// Remove an existing repository
    Remove {
        /// Repository alias to remove
        alias: String,
    },
    /// Clear all repositories
    Clear,
}

#[derive(Debug, Subcommand)]
pub enum DependencyOperation {
    /// Add a dependency
    Add {
        /// Name of the dependency to add
        name: String,

        /// Alias of the repository to specify
        #[clap(long, conflicts_with_all = ["git", "url", "path"])]
        repository: Option<String>,

        /// Enable force_source
        #[clap(long, conflicts_with_all = ["no_force_source", "url", "path", "git"])]
        force_source: bool,
        /// Enable force_source = false. This will override repository level force_source = true for this package
        #[clap(long, conflicts_with_all = ["force_source", "url", "path", "git"])]
        no_force_source: bool,

        /// Direct HTTP URL from which to install from
        #[clap(long, conflicts_with_all = ["git", "repository", "path"])]
        url: Option<Url>,

        /// Local path from which to install from
        #[clap(long, conflicts_with_all = ["git", "repository", "url"])]
        path: Option<PathBuf>,

        /// A git repository URL (SSH or HTTP) - requires one and only one of branch, tag, or commit.
        #[clap(long, conflicts_with_all = ["url", "repository", "path"])]
        git: Option<String>,
        /// Git Reference: branch name. (Requires --git; conflicts with tag, commit)
        #[clap(long, requires = "git", conflicts_with_all = ["tag", "commit"])]
        branch: Option<String>,
        /// Git Reference: tag name. (Requires --git; conflicts with branch, commit)
        #[clap(long, requires = "git", conflicts_with_all = ["branch", "commit"])]
        tag: Option<String>,
        /// Git Reference: exact commit sha. (Requires --git; conflicts with branch, tag)
        #[clap(long, requires = "git", conflicts_with_all = ["branch", "tag"])]
        commit: Option<String>,
        /// Optional subdirectory within the git repo to use as the package root
        #[clap(long, requires = "git")]
        directory: Option<PathBuf>,

        /// Enable the install_suggestions option
        #[clap(long, short = 's')]
        install_suggestions: bool,

        /// Enable the dependencies_only option
        #[clap(long, short = 'd')]
        dependencies_only: bool,
    },
    /// Remove an existing dependency
    Remove {
        /// Name of the dependencies to remove
        name: String,
    },
    /// Clear all dependencies
    Clear,
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

#[derive(Debug, Clone, PartialEq)]
enum OutputFormat {
    Json,
    Plain,
}

impl OutputFormat {
    fn is_json(&self) -> bool {
        matches!(self, OutputFormat::Json)
    }
}

/// Resolve dependencies for the project. If there are any unmet dependencies, they will be printed
/// to stderr and the cli will exit.
fn resolve_dependencies<'a>(
    context: &'a CliContext,
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

#[derive(Debug, Default, Serialize)]
struct SyncChanges {
    installed: Vec<SyncChange>,
    removed: Vec<SyncChange>,
}

impl SyncChanges {
    fn from_changes(changes: Vec<SyncChange>) -> Self {
        let mut installed = vec![];
        let mut removed = vec![];
        for change in changes {
            if change.installed {
                installed.push(change);
            } else {
                removed.push(change);
            }
        }
        Self { installed, removed }
    }
}

fn _sync(
    mut context: CliContext,
    dry_run: bool,
    has_logs_enabled: bool,
    resolve_mode: ResolveMode,
    output_format: OutputFormat,
    save_install_logs_in: Option<PathBuf>,
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
    context.load_system_requirements()?;

    let resolved = resolve_dependencies(&context, &resolve_mode, true).found;

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
                &context.system_dependencies,
                save_install_logs_in.clone(),
                context.staging_path(),
            );
            if dry_run {
                handler.dry_run();
            }
            if !has_logs_enabled {
                handler.show_progress_bar();
            }
            handler.set_uses_lockfile(context.config.use_lockfile());
            handler.handle(&resolved, &context.r_cmd)
        }
    ) {
        Ok(mut changes) => {
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
            let all_sys_deps: HashSet<_> = changes
                .iter()
                .flat_map(|x| x.sys_deps.iter().map(|x| x.name.as_str()))
                .collect();
            let sysdeps_status =
                system_req::check_installation_status(&context.cache.system_info, &all_sys_deps);

            for change in changes.iter_mut() {
                change.update_sys_deps_status(&sysdeps_status);
            }

            if output_format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&SyncChanges::from_changes(changes,))
                        .expect("valid json")
                );
            } else if changes.is_empty() {
                println!("Nothing to do");
            } else {
                for c in changes {
                    println!("{}", c.print(!dry_run, !sysdeps_status.is_empty()));
                }
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
    let output_format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Plain
    };
    let log_enabled = cli.verbose.is_present() && !output_format.is_json();
    env_logger::Builder::new()
        .filter_level(if cli.json {
            log::LevelFilter::Off
        } else {
            cli.verbose.log_level_filter()
        })
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
                // if R version is not provided, get the major.minor of the R version on the path
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
                match find_r_repositories() {
                    Ok(repos) if !repos.is_empty() => repos,
                    _ => {
                        eprintln!(
                            "WARNING: Could not set default repositories. Set with your company preferred package URL or public url (i.e. `https://packagemanager.posit.co/cran/latest`)\n"
                        );
                        Vec::new()
                    }
                }
            };

            init(&project_directory, &r_version, &repositories, &add, force)?;
            activate(&project_directory, no_r_environment)?;

            if output_format.is_json() {
                println!(
                    "{}",
                    json!({"directory": format!("{}", project_directory.display())})
                );
            } else {
                println!(
                    "rv project successfully initialized at {}",
                    project_directory.display()
                );
            }
        }
        Command::Library => {
            let context = CliContext::new(&cli.config_file, RCommandLookup::Skip)?;
            let path_str = context.library_path().to_string_lossy();
            let path_out = if cfg!(windows) {
                path_str.replace('\\', "/")
            } else {
                path_str.to_string()
            };

            if output_format.is_json() {
                println!("{}", json!({"directory": path_out}));
            } else {
                println!("{path_out}");
            }
        }
        Command::Plan { upgrade, r_version } => {
            let upgrade = if upgrade || r_version.is_some() {
                ResolveMode::FullUpgrade
            } else {
                ResolveMode::Default
            };
            let context = CliContext::new(&cli.config_file, r_version.into())?;
            _sync(context, true, log_enabled, upgrade, output_format, None)?;
        }
        Command::Sync {
            save_install_logs_in,
        } => {
            let context = CliContext::new(&cli.config_file, RCommandLookup::Strict)?;
            _sync(
                context,
                false,
                log_enabled,
                ResolveMode::Default,
                output_format,
                save_install_logs_in,
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
                if output_format.is_json() {
                    // Nothing to output for JSON format here since we didn't sync anything
                    println!("{{}}");
                } else {
                    println!("Packages successfully added");
                }
                return Ok(());
            }
            let mut context = CliContext::new(&cli.config_file, RCommandLookup::Strict)?;
            // if dry run, the config won't have been edited to reflect the added changes so must be added
            if dry_run {
                context.config = doc.to_string().parse::<Config>()?;
            }
            _sync(
                context,
                dry_run,
                log_enabled,
                ResolveMode::Default,
                output_format,
                None,
            )?;
        }
        Command::Upgrade { dry_run } => {
            let context = CliContext::new(&cli.config_file, RCommandLookup::Strict)?;
            _sync(
                context,
                dry_run,
                log_enabled,
                ResolveMode::FullUpgrade,
                output_format,
                None,
            )?;
        }
        Command::Info {
            library,
            r_version,
            repositories,
        } => {
            // TODO: handle info, eg need to accumulate fields
            let mut output = Vec::new();
            let context = CliContext::new(&cli.config_file, RCommandLookup::Skip)?;
            if library {
                let path_str = context.library_path().to_string_lossy();
                let path_out = if cfg!(windows) {
                    path_str.replace('\\', "/")
                } else {
                    path_str.to_string()
                };
                output.push(("library", path_out));
            }
            if r_version {
                output.push(("r-version", context.r_version.original.to_owned()));
            }
            if repositories {
                let repos = context
                    .config
                    .repositories()
                    .iter()
                    .map(|r| format!("({}, {})", r.alias, r.url()))
                    .collect::<Vec<_>>()
                    .join(", ");
                output.push(("repositories", repos));
            }

            if output_format.is_json() {
                let output: HashMap<_, _> = output.into_iter().collect();
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                for (key, val) in output {
                    println!("{key}: {val}");
                }
            }
        }
        Command::Cache => {
            let mut context = CliContext::new(&cli.config_file, RCommandLookup::Skip)?;
            context.load_databases()?;
            if !log_enabled {
                context.show_progress_bar();
            }
            let info = CacheInfo::new(
                &context.config,
                &context.cache,
                resolve_dependencies(&context, &ResolveMode::Default, true).found,
            );
            if output_format.is_json() {
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
                if output_format.is_json() {
                    println!(
                        "{}",
                        json!({
                            "success": true,
                            "unresolved": [],
                        })
                    );
                } else {
                    println!(
                        "{} was successfully migrated to {}",
                        renv_file.display(),
                        cli.config_file.display()
                    );
                }
            } else if output_format.is_json() {
                println!(
                    "{}",
                    json!({
                        "success": false,
                        "unresolved": unresolved.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    })
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
        Command::Summary { r_version } => {
            let mut context = CliContext::new(&cli.config_file, r_version.into())?;
            context.load_databases()?;
            context.load_system_requirements()?;
            if !log_enabled {
                context.show_progress_bar();
            }
            let resolved = resolve_dependencies(&context, &ResolveMode::Default, true).found;
            let project_sys_deps: HashSet<_> = resolved
                .iter()
                .flat_map(|x| context.system_dependencies.get(x.name.as_ref()))
                .flatten()
                .map(|x| x.as_str())
                .collect();

            let sys_deps: Vec<_> = system_req::check_installation_status(
                &context.cache.system_info,
                &project_sys_deps,
            )
            .into_iter()
            .map(|(name, status)| SysDep { name, status })
            .collect();

            let summary = ProjectSummary::new(
                &context.library,
                &resolved,
                context.config.repositories(),
                &context.databases,
                &context.r_version,
                &context.cache,
                context.lockfile.as_ref(),
                sys_deps,
            );
            if output_format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&summary).expect("valid json")
                );
            } else {
                println!("{summary}");
            }
        }
        Command::Activate { no_r_environment } => {
            let context = CliContext::new(&cli.config_file, RCommandLookup::Skip)?;
            activate(&context.project_dir, no_r_environment)?;
            if output_format.is_json() {
                println!("{{}}");
            } else {
                println!("rv activated");
            }
        }
        Command::Deactivate => {
            let context = CliContext::new(&cli.config_file, RCommandLookup::Skip)?;
            deactivate(&context.project_dir)?;
            if output_format.is_json() {
                println!("{{}}");
            } else {
                println!("rv deactivated");
            }
        }
        Command::Sysdeps {
            only_absent,
            ignore,
        } => {
            let mut context = CliContext::new(&cli.config_file, RCommandLookup::Skip)?;
            if !log_enabled {
                context.show_progress_bar();
            }
            context.load_databases_if_needed()?;
            context.load_system_requirements()?;

            let resolved = resolve_dependencies(&context, &ResolveMode::Default, false).found;
            let project_sys_deps: HashSet<_> = resolved
                .iter()
                .flat_map(|x| context.system_dependencies.get(x.name.as_ref()))
                .flatten()
                .map(|x| x.as_str())
                .collect();

            let sys_deps_status = system_req::check_installation_status(
                &context.cache.system_info,
                &project_sys_deps,
            );

            let mut sys_deps_names: Vec<_> = sys_deps_status
                .into_iter()
                .filter(|(name, status)| {
                    // Filter by only_absent flag
                    if only_absent && *status != SysInstallationStatus::Absent {
                        return false;
                    }

                    // Filter by ignore list
                    !ignore.contains(name)
                })
                .map(|(name, _)| name)
                .collect();

            // Sort by name for consistent output
            sys_deps_names.sort_by(|a, b| a.cmp(&b));

            if output_format.is_json() {
                println!("{}", json!(sys_deps_names));
            } else {
                for name in &sys_deps_names {
                    println!("{name}");
                }
            }
        }

        Command::Tree {
            depth,
            hide_system_deps,
            r_version,
        } => {
            let mut context = CliContext::new(&cli.config_file, r_version.into())?;
            context.load_databases_if_needed()?;
            if !hide_system_deps {
                context.load_system_requirements()?;
            }
            if !log_enabled {
                context.show_progress_bar();
            }
            let resolution = resolve_dependencies(&context, &ResolveMode::Default, false);
            let tree = tree(&context, &resolution.found, &resolution.failed);

            if output_format.is_json() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&tree).expect("valid json")
                );
            } else {
                tree.print(depth, !hide_system_deps);
            }
        }
        Command::Configure { subcommand } => {
            match subcommand {
                ConfigureSubcommand::Repository { operation } => {
                    let action = match operation {
                        RepositoryOperation::Clear => RepositoryAction::Clear,

                        RepositoryOperation::Remove { alias } => RepositoryAction::Remove { alias },

                        RepositoryOperation::Add {
                            alias,
                            url,
                            force_source,
                            first,
                            last,
                            before,
                            after,
                        } => {
                            let parsed_url = url::Url::parse(&url)
                                .map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?;

                            let positioning = if first {
                                RepositoryPositioning::First
                            } else if last {
                                RepositoryPositioning::Last
                            } else if let Some(before_alias) = before {
                                RepositoryPositioning::Before(before_alias)
                            } else if let Some(after_alias) = after {
                                RepositoryPositioning::After(after_alias)
                            } else {
                                RepositoryPositioning::Last // Default
                            };

                            RepositoryAction::Add {
                                alias,
                                url: parsed_url,
                                positioning,
                                force_source,
                            }
                        }

                        RepositoryOperation::Replace {
                            old_alias,
                            alias,
                            url,
                            force_source,
                        } => {
                            let parsed_url = url::Url::parse(&url)
                                .map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?;
                            let new_alias = alias.unwrap_or_else(|| old_alias.clone());

                            RepositoryAction::Replace {
                                old_alias,
                                new_alias,
                                url: parsed_url,
                                force_source,
                            }
                        }

                        RepositoryOperation::Update {
                            target_alias,
                            match_url,
                            alias,
                            url,
                            force_source,
                            no_force_source,
                        } => {
                            // Determine matcher
                            let matcher = if let Some(match_url_str) = match_url {
                                let parsed_url = url::Url::parse(&match_url_str)
                                    .map_err(|e| anyhow::anyhow!("Invalid match URL: {}", e))?;
                                RepositoryMatcher::ByUrl(parsed_url)
                            } else if let Some(target_alias) = target_alias {
                                RepositoryMatcher::ByAlias(target_alias)
                            } else {
                                return Err(anyhow::anyhow!(
                                    "Must specify either target alias or --match-url"
                                ));
                            };

                            // Parse URL if provided
                            let parsed_url = if let Some(url_str) = url {
                                Some(
                                    url::Url::parse(&url_str)
                                        .map_err(|e| anyhow::anyhow!("Invalid URL: {}", e))?,
                                )
                            } else {
                                None
                            };

                            // Determine force_source value
                            let force_source_update = if force_source {
                                Some(true)
                            } else if no_force_source {
                                Some(false)
                            } else {
                                None
                            };

                            let updates = RepositoryUpdates {
                                alias,
                                url: parsed_url,
                                force_source: force_source_update,
                            };

                            RepositoryAction::Update { matcher, updates }
                        }
                    };

                    let response = execute_repository_action(&cli.config_file, action)?;

                    // Handle output based on format preference
                    if output_format.is_json() {
                        println!("{}", serde_json::to_string_pretty(&response)?);
                    } else {
                        // Print detailed text output
                        match response.operation {
                            LibRepositoryOperation::Add => {
                                println!(
                                    "Repository '{}' added successfully with URL: {}",
                                    response.alias.as_ref().unwrap(),
                                    response.url.as_ref().unwrap()
                                );
                            }
                            LibRepositoryOperation::Replace => {
                                println!(
                                    "Repository replaced successfully - new alias: '{}', URL: {}",
                                    response.alias.as_ref().unwrap(),
                                    response.url.as_ref().unwrap()
                                );
                            }
                            LibRepositoryOperation::Update => {
                                println!(
                                    "Repository '{}' updated successfully",
                                    response.alias.as_ref().unwrap()
                                );
                            }
                            LibRepositoryOperation::Remove => {
                                println!(
                                    "Repository '{}' removed successfully",
                                    response.alias.as_ref().unwrap()
                                );
                            }
                            LibRepositoryOperation::Clear => {
                                println!("All repositories cleared successfully");
                            }
                        }
                    }
                }
                ConfigureSubcommand::Dependency { operation } => {
                    let action = match operation {
                        DependencyOperation::Add {
                            name,
                            repository,
                            force_source,
                            no_force_source,
                            url,
                            path,
                            git,
                            branch,
                            tag,
                            commit,
                            directory,
                            install_suggestions,
                            dependencies_only,
                        } => {
                            let dependency_type = if let Some(url) = url {
                                DependencyType::Url(url.try_into().map_err(|e| anyhow!("{e}"))?)
                            } else if let Some(path) = path {
                                DependencyType::Local(path)
                            } else if let Some(git) = git {
                                let git = git.as_str().try_into().map_err(|e| anyhow!("{e}"))?;
                                let reference = GitDepRef::try_new(tag, branch, commit)
                                    .map_err(|e| anyhow!("{e}"))?;
                                DependencyType::Git {
                                    git,
                                    reference,
                                    directory,
                                }
                            } else {
                                let force_source = match (force_source, no_force_source) {
                                    (false, false) => None,
                                    (true, false) => Some(true),
                                    (false, true) => Some(false),
                                    (true, true) => unreachable!("conflicts in clap subcommand")
                                };

                                if force_source.is_none() && repository.is_none() {
                                    DependencyType::Simple
                                } else {
                                    DependencyType::Detailed { repository, force_source }
                                }
                            };

                            DependencyAction::Add {
                                name,
                                dependency_type,
                                install_suggestions,
                                dependencies_only,
                            }
                        }
                        DependencyOperation::Remove { name } => DependencyAction::Remove { name: name },
                        DependencyOperation::Clear => DependencyAction::Clear,
                    };

                    let response = action.execute_action(&cli.config_file)?;
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
