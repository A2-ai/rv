use std::{fs::File, io::Write, path::Path};

use anyhow::Result;

use crate::{
    cli::{context::load_databases, DiskCache},
    renv::ResolvedRenv,
    RenvLock, Repository, SystemInfo, Version,
};

const RENV_CONFIG_TEMPLATE: &str = r#"# this config was migrated from %renv_file% on %time%
[project]
name = "%project_name%"
r_version = "%r_version%"

repositories = [
%repositories%
]

dependencies = [
%dependencies%
]
"#;

pub fn migrate_renv(renv_file: impl AsRef<Path>, config_file: impl AsRef<Path>) -> Result<()> {
    // project name is the parent directory of the renv project
    let project_name = renv_file
        .as_ref()
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("renv migrated project");

    // use the repositories and r version from the renv.lock to determine the repository databases
    let renv_lock = RenvLock::parse_renv_lock(&renv_file)?;
    let cache = DiskCache::new(renv_lock.r_version(), SystemInfo::from_os_info())?;
    let databases = load_databases(&renv_lock.repositories(), &cache)?;

    // resolve the renv.lock file to determine the true source of packages
    let (resolved, unresolved) = renv_lock.resolve(&databases);

    // if the source cannot be determined, we DO NOT error.
    // This tool is an aide to help migrate most packages, but some manual migration may be needed
    for u in unresolved {
        eprintln!("{u}");
    }

    // Write config out to the config file specified in the cli, even if config file is outside of the renv.lock project
    let config = render_config(
        &renv_file.as_ref().to_string_lossy(),
        project_name,
        renv_lock.r_version(),
        &renv_lock.repositories(),
        &resolved,
    );
    let mut file = File::create(&config_file)?;
    file.write_all(config.as_bytes())?;
    println!(
        "{} was successfully migrated to {}",
        renv_file.as_ref().display(),
        config_file.as_ref().display()
    );
    Ok(())
}

fn render_config(
    renv_file: &str,
    project_name: &str,
    r_version: &Version,
    repositories: &Vec<Repository>,
    resolved_deps: &Vec<ResolvedRenv>,
) -> String {
    let repos = repositories
        .iter()
        .map(|r| format!("    {r}"))
        .collect::<Vec<_>>()
        .join(",\n");

    let deps = resolved_deps
        .iter()
        .map(|d| format!("    {d}"))
        .collect::<Vec<_>>()
        .join(",\n");

    // get time. Try to round to seconds, but if error, leave as unrounded
    let time = jiff::Zoned::now();
    let time = time.round(jiff::Unit::Second).unwrap_or(time);

    RENV_CONFIG_TEMPLATE
        .replace("%renv_file%", renv_file)
        .replace("%time%", &time.to_string())
        .replace("%project_name%", project_name)
        .replace("%r_version%", &r_version.original)
        .replace("%repositories%", &repos)
        .replace("%dependencies%", &deps)
}
