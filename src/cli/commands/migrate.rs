use std::{fs::File, io::Write, path::Path};

use crate::{
    cli::{context::load_databases, DiskCache},
    renv::ResolvedRenv,
    RenvLock, Repository, SystemInfo, Version,
};
use anyhow::Result;

const RENV_CONFIG_TEMPLATE: &str = r#"# This project was migrate from %renv_file% on %time%
[project]
name = %project_name%
r_version = %r_version%

repositories = [
%repositories%
]

dependencies = [
%dependencies%
]
"#;

pub fn migrate_renv(renv_file: impl AsRef<Path>, config_file: impl AsRef<Path>) -> Result<()> {
    let renv_file = renv_file.as_ref();

    // project name will is the folder in which the renv.lock file is in
    let project_name = renv_file
        .parent()
        .and_then(|s| s.to_str())
        .unwrap_or("migrated renv project")
        .to_string();

    // parse renv.lock file. Need r version to get cache and repositories to load repository database
    let renv_lock = RenvLock::parse_renv_lock(&renv_file)?;
    let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info())?;
    let databases = load_databases(&renv_lock.repositories(), &cache)?;

    // resolve renv.lock
    let (resolved, unresolved) = renv_lock.resolve(&databases);

    // print out any unresolved packages. Failure to resolve a package DOES NOT result in an error
    // Migration is to resolve as many packages as possible to provide user with a starting point
    // Manual resolution of unresolved packages can be done within the config file
    for u in unresolved {
        eprintln!("{u}")
    }

    let config = render_config(
        project_name,
        renv_lock.r_version(),
        &renv_lock.repositories(),
        &resolved,
    );

    // we respect the Cli config_file variable. Users may want to name their config file and/or use migrate for renv.lock analysis
    let mut file = File::create(config_file)?;
    file.write_all(config.as_bytes())?;
    Ok(())
}

fn render_config(
    project_name: String,
    r_version: &Version,
    repositories: &Vec<Repository>,
    dependencies: &Vec<ResolvedRenv>,
) -> String {
    let repos = repositories
        .iter()
        .map(|r| format!(r#"    {{alias = {}, url = {}}}"#, r.alias, r.url()))
        .collect::<Vec<_>>()
        .join(",/n");
    let deps = dependencies
        .iter()
        .map(|r| r.as_formatted_string())
        .collect::<Vec<_>>()
        .join(",/n");

    RENV_CONFIG_TEMPLATE
        .replace("%project_name%", &project_name)
        .replace("%r_version%", &r_version.original)
        .replace("%repositories%", &repos)
        .replace("%dependencies%", &deps)
}
