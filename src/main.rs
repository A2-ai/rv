use clap::{Parser, Subcommand};
use env_logger::Env;
use rv::{
    cli::install::{execute_install, InstallArgs},
    execute_plan, Config, Distribution, PlanArgs,
};
use std::path::PathBuf;

// use rand::Rng;
#[derive(Parser)]
#[clap(version, author, about, subcommand_negates_reqs = true)]
pub struct Cli {
    /// Do not print any output
    #[clap(long, default_value_t = false)]
    pub quiet: bool,

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
    Plan {
        /// Specify the R version (e.g., 4.3, 4.4.1)
        #[clap(long, value_parser)]
        r_version: Option<String>,

        /// Specify the system distribution (e.g., jammy, mac)
        #[clap(long, value_enum)]
        distribution: Option<Distribution>,
    },
    /// Replaces the library with exactly what is in the lock file
    Sync,
    /// Install a package
    Install {
        /// Destination directory where the archive will be extracted
        destination: PathBuf,
    },
}

fn try_main() {
    let cli = Cli::parse();
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();
    // TODO: parse config file here and fetch R version if needed
    // except for init
    // let config = Config::from_file(&cli.config_file);

    let config = Config::from_file(&cli.config_file);
    match cli.command {
        Command::Install { destination } => {
            execute_install(&config, InstallArgs { destination });
        }
        Command::Init => todo!("implement init"),
        Command::Plan {
            r_version,
            distribution,
        } => {
            execute_plan(
                &config,
                PlanArgs {
                    r_version_str: r_version,
                    distribution,
                },
            );
        }
        Command::Sync => todo!("implement sync"),
    }
}

fn main() {
    try_main()
}
