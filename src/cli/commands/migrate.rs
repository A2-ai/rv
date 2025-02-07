use std::{fmt::format, fs::File, io::Write, path::Path};

use crate::{cli::{context::load_databases, DiskCache}, renv::ResolvedRenv, RenvLock, Repository, Version};

const RENV_CONFIG_TEMPLATE: &str = r#"this config was migrated from %renv_file% on %time%
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

fn migrate_renv(renv_file: impl AsRef<Path>, config_file: impl AsRef<Path>) -> Result<()> {
    let project_name = renv_file.as_ref().parent().and_then(|p| p.to_str()).unwrap_or("renv migrated project");
    let renv_lock = RenvLock::parse_renv_lock(renv_file)?;
    let cache = DiskCache::new(renv_file.r_version, system_info)?;
    let databases = load_databases(&renv_lock.repositories(), &cache)?;
    let (resolved, unresolved) = renv_lock.resolve(&databases);
    for u in unresolved {
        eprintln!("{u}");
    }

    let config = render_config(renv_file, project_name, r_version, repositories, resolved_deps);

    let mut file = File::create(config_file)?;
    file.write_all(config.bytes())?;

    Ok(())
}

fn render_config(renv_file: &str, project_name: &str, r_version: &Version, repositories: &Vec<Repository>, resolved_deps: &Vec<ResolvedRenv>) -> String {
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

    let time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    RENV_CONFIG_TEMPLATE
        .replace("%renv_file%", renv_file)
        .replace("%time%", &time)
        .replace("%project_name%", project_name)
        .replace("%r_version%", &r_version.original)
        .replace("%repositories%", &repos)
        .replace("%dependencies%", &deps)
}