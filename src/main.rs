use clap::{Parser, Subcommand};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::Result;
use fs_err::{read_to_string, write};
use serde_json::json;

use rv::RepositoryOperation as LibRepositoryOperation;
use rv::cli::{
    CliContext, OutputFormat, RCommandLookup, ResolveMode, SyncHelper, find_r_repositories, init,
    init_structure, migrate_renv, resolve_dependencies, tree,
};
use rv::system_req::{SysDep, SysInstallationStatus};
use rv::{
    CacheInfo, Config, ProjectSummary, RCmd, RCommandLine, RepositoryAction, RepositoryMatcher,
    RepositoryPositioning, RepositoryUpdates, Version, activate, add_packages, deactivate,
    execute_repository_action, read_and_verify_config, system_req,
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
    /// Migrate renv to rv
    Migrate {
        #[clap(subcommand)]
        subcommand: MigrateSubcommand,
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
    /// Upgrade packages to the latest versions available
    Upgrade {
        #[clap(long)]
        dry_run: bool,
    },
    /// Dry run of what sync would do
    Plan {
        #[clap(short, long)]
        upgrade: bool,
        /// Specify a R version different from the one in the config.
        /// The command will not error even if this R version is not found
        #[clap(long)]
        r_version: Option<Version>,
    },
    /// Provide a summary about the project status
    Summary {
        /// Specify a R version different from the one in the config.
        /// The command will not error even if this R version is not found
        #[clap(long)]
        r_version: Option<Version>,
    },
    /// Configure project settings
    Configure {
        #[command(subcommand)]
        subcommand: ConfigureSubcommand,
    },
    /// Formats the toml configuration file while preserving comments and spacing
    Fmt {
        // add a --check flag to check formatting without changing the file
        /// check the formatting without changing the file
        #[clap(long)]
        check: bool,
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
    /// Returns the path for the library for the current project/system in UNIX format, even
    /// on Windows.
    Library,
    /// Gives information about where the cache is for that project
    Cache,
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
    /// Activate a previously initialized rv project
    Activate {
        #[clap(long)]
        no_r_environment: bool,
    },
    /// Deactivate an rv project
    Deactivate,
}

#[derive(Debug, Subcommand)]
pub enum ConfigureSubcommand {
    /// Configure project repositories
    Repository {
        #[clap(subcommand)]
        operation: RepositoryOperation,
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
        Command::Sync {
            save_install_logs_in,
        } => {
            let mut context = CliContext::new(&cli.config_file, RCommandLookup::Strict)?;

            if !log_enabled {
                context.show_progress_bar();
            }
            let resolve_mode = ResolveMode::Default;
            context.load_for_resolve_mode(resolve_mode)?;
            SyncHelper {
                dry_run: false,
                output_format: Some(output_format),
                save_install_logs_in,
                ..Default::default()
            }
            .run(&context, resolve_mode)?;
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

            if !log_enabled {
                context.show_progress_bar();
            }
            // if dry run, the config won't have been edited to reflect the added changes so must be added
            if dry_run {
                context.config = doc.to_string().parse::<Config>()?;
            }
            let resolve_mode = ResolveMode::Default;
            context.load_for_resolve_mode(resolve_mode)?;
            SyncHelper {
                dry_run,
                output_format: Some(output_format),
                ..Default::default()
            }
            .run(&context, resolve_mode)?;
        }
        Command::Upgrade { dry_run } => {
            let mut context = CliContext::new(&cli.config_file, RCommandLookup::Strict)?;

            if !log_enabled {
                context.show_progress_bar();
            }
            let resolve_mode = ResolveMode::FullUpgrade;
            context.load_for_resolve_mode(resolve_mode)?;
            SyncHelper {
                dry_run,
                output_format: Some(output_format),
                ..Default::default()
            }
            .run(&context, resolve_mode)?;
        }
        Command::Plan { upgrade, r_version } => {
            let upgrade = if upgrade || r_version.is_some() {
                ResolveMode::FullUpgrade
            } else {
                ResolveMode::Default
            };
            let mut context = CliContext::new(&cli.config_file, r_version.into())?;

            if !log_enabled {
                context.show_progress_bar();
            }
            context.load_for_resolve_mode(upgrade)?;
            SyncHelper {
                dry_run: true,
                output_format: Some(output_format),
                ..Default::default()
            }
            .run(&context, upgrade)?;
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
        // configure left at bottom due to its size
        Command::Fmt { check } => {
            let contents = read_to_string(&cli.config_file)?;
            let formatted = rv::format_document(&contents);
            if contents == formatted {
                if output_format.is_json() {
                    println!("{{\"reformat\": false}}");
                } else {
                    println!("Config file is already formatted");
                }
                return Ok(());
            }
            // if we've gotten here we weren't formatted, so if check we bail
            // otherwise we rewrite the file
            if check {
                eprintln!("Config file is not formatted correctly");
                ::std::process::exit(1);
            } else {
                write(&cli.config_file, formatted)?;
                if output_format.is_json() {
                    println!("{{\"reformat\": true}}");
                } else {
                    println!("Config file successfully formatted");
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
        Command::Activate { no_r_environment } => {
            let config_file = cli.config_file.canonicalize()?;
            let project_dir = config_file.parent().expect("parent to exist");
            activate(project_dir, no_r_environment)?;
            if output_format.is_json() {
                println!("{{}}");
            } else {
                println!("rv activated");
            }
        }
        Command::Deactivate => {
            let config_file = cli.config_file.canonicalize()?;
            let project_dir = config_file.parent().expect("parent to exist");
            deactivate(project_dir)?;
            if output_format.is_json() {
                println!("{{}}");
            } else {
                println!("rv deactivated");
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
