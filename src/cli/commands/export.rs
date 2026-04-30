use std::path::Path;

use anyhow::{Result, anyhow};

use crate::{Config, Lockfile, renv::to_renv_lock};

pub fn export_renv(config_file: &Path, output_file: &Path) -> Result<Vec<String>> {
    let config = Config::from_file(config_file).map_err(|e| anyhow!("{e}"))?;

    let project_dir = config_file
        .parent()
        .ok_or_else(|| anyhow!("Could not determine project directory from config file path"))?;
    let lockfile_path = project_dir.join(config.lockfile_name());
    let lockfile = Lockfile::load(&lockfile_path)
        .map_err(|e| anyhow!("{e}"))?
        .ok_or_else(|| anyhow!("No valid lockfile found at {}", lockfile_path.display()))?;

    let (renv_json, warnings) = to_renv_lock(&lockfile, &config);

    let json_string = serde_json::to_string_pretty(&renv_json)?;
    fs_err::write(output_file, json_string)?;

    Ok(warnings)
}
