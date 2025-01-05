use clap::{Parser, Subcommand};
use env_logger::Env;
use rv::{
    cli::install::{execute_install, InstallArgs},
    cli::plan::{execute_plan, Distribution, PlanArgs},
    Config,
};
use std::io::Write;
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
    // if RV_LOG is set, then we want to use the RV_LOG setup
    // otherwise we want to remove the leveled/structure components of the log and just output the args
    if std::env::var("RV_LOG").is_ok() {
        let env = Env::new().filter("RV_LOG");
        env_logger::init_from_env(env);
    } else {
        env_logger::builder()
            .parse_filters("info")
            .format(|buf, record| writeln!(buf, "{}", record.args()))
            .init();
    }
    // TODO: parse config file here and fetch R version if needed
    // except for init
    // let config = Config::from_file(&cli.config_file);

    let config = Config::from_file(&cli.config_file);
    match cli.command {
        Command::Install { destination } => {
            // create the destination if it doesn't exist
            if !destination.exists() {
                std::fs::create_dir_all(&destination).expect("Failed to create destination");
            }
            execute_install(&config, &destination);
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
