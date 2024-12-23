use clap::{Parser, Subcommand};
use std::path::PathBuf;

use rv::{Config, RCommandLine, RepositoryDatabase, Resolver};

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
    Plan,
    /// Replaces the library with exactly what is in the lock file
    Sync,
}

fn try_main() {
    let cli = Cli::parse();

    // TODO: parse config file here and fetch R version if needed
    // except for init
    // let config = Config::from_file(&cli.config_file);

    match cli.command {
        Command::Init => todo!("implement init"),
        Command::Plan => {
            let config = Config::from_file(&cli.config_file);
            let r_cli = RCommandLine {};
            let r_version = config.get_r_version(r_cli);

            let databases: Vec<_> = config
                .repositories()
                .iter()
                .map(|r| {
                    // 1. Generate path to add to URL to get the src PACKAGE and binary PACKAGE for current OS
                    // 2. Check in cache whether we have the database and is not expired
                    // 3. Fetch the PACKAGE files if needed and build the database + persist to disk

                    // For now mocking the repositories database generation/loading until we get
                    // the paths PR merged + some basic caching
                    let mut db = RepositoryDatabase::new(&r.alias);
                    let content = std::fs::read_to_string(format!(
                        "src/tests/package_files/posit-src.PACKAGE"
                    ))
                    .unwrap();
                    db.parse_source(&content);
                    let content = std::fs::read_to_string(format!(
                        "src/tests/package_files/cran-binary.PACKAGE"
                    ))
                    .unwrap();
                    db.parse_binary(&content, &r_version);
                    db
                })
                .collect();

            let resolver = Resolver::new(&databases, &r_version);
            let (resolved, unresolved) = resolver.resolve(config.dependencies());

            // TODO: later differentiate packages that need to be downloaded from packages
            // already cached
            if unresolved.is_empty() {
                println!("Plan successful! The following packages will be installed:");
                for d in resolved {
                    println!("    {d}");
                }
            } else {
                eprintln!("Failed to find all dependencies");
                for d in unresolved {
                    println!("    {d}");
                }
            }
        }
        Command::Sync => todo!("implement sync"),
    }
}

fn main() {
    try_main()
}
