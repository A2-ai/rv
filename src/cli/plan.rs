use clap::ValueEnum;
use std::{collections::HashMap, path::PathBuf, str::FromStr};

use crate::{
    cli::DiskCache,
    db::load_databases,
    install::get_installed_pkgs,
    Config,
    RCommandLine,
    Resolver,
    SystemInfo,
    Version, // anything else needed...
};
use log::{debug, error, info, trace};

#[derive(Debug, Clone, PartialEq, Eq, ValueEnum)]
pub enum Distribution {
    Mac,
    Windows,
    Focal,
    Jammy,
    Noble,
}
/// Holds any additional arguments we need to pass to plan logic
pub struct PlanArgs {
    pub r_version_str: Option<String>,
    pub distribution: Option<Distribution>,
    pub destination: Option<PathBuf>,
}

pub fn execute_plan(config: &Config, plan_args: PlanArgs) {
    let total_start_time = std::time::Instant::now();
    let r_cli = RCommandLine {};
    let no_user_override = plan_args.r_version_str.is_none() && plan_args.distribution.is_none();

    // Determine the R version
    let mut start_time = std::time::Instant::now();
    let r_version = match plan_args.r_version_str {
        Some(ver_str) => Version::from_str(&ver_str).expect("Invalid R version format"),
        None => config.get_r_version(r_cli),
    };
    trace!("time to get r version: {:?}", start_time.elapsed());

    // Determine the distribution and SystemInfo
    start_time = std::time::Instant::now();
    let sysinfo = match plan_args.distribution {
        Some(Distribution::Mac) => {
            // Example for Mac
            SystemInfo::new(crate::OsType::MacOs, Some("aarch64".into()), None, "12.0")
        }
        Some(Distribution::Windows) => {
            // Example for Windows
            SystemInfo::new(crate::OsType::Windows, Some("x86_64".into()), None, "12.0")
        }
        Some(dist) => {
            // Example for Ubuntu-based
            let (os_type, codename, release) = match dist {
                Distribution::Focal => ("ubuntu", "focal", "20.04"),
                Distribution::Jammy => ("ubuntu", "jammy", "22.04"),
                Distribution::Noble => ("ubuntu", "noble", "24.04"),
                _ => unreachable!(),
            };
            SystemInfo::new(
                crate::OsType::Linux(os_type),
                Some("x86_64".into()),
                Some(codename.to_string()),
                release,
            )
        }
        None => SystemInfo::from_os_info(),
    };
    trace!("time to get sysinfo: {:?}", start_time.elapsed());

    // Load databases
    start_time = std::time::Instant::now();
    let cache = DiskCache::new(&r_version, sysinfo.clone());
    let databases = load_databases(config.repositories(), &cache, &r_version, no_user_override);
    debug!("Loading databases took: {:?}", start_time.elapsed());

    // Resolve
    start_time = std::time::Instant::now();
    let resolver = Resolver::new(&databases, &r_version);
    let (resolved, unresolved) = resolver.resolve(config.dependencies());
    trace!("Resolving took: {:?}", start_time.elapsed());
    if !unresolved.is_empty() {
        error!("Failed to find all dependencies");
        for d in unresolved {
            info!("    {d}");
        }
        return;
    }
    info!("Plan successful! The following packages will be installed:");
    let installed_pkgs = match plan_args.destination {
        None => HashMap::new(),
        Some(dest) => get_installed_pkgs(dest.as_path()).unwrap(),
    };
    // Categorize packages into installed (correct version), needs update, and missing
    let (installed, needs_action): (Vec<_>, Vec<_>) = resolved.into_iter()
        .partition(|dep| {
            if let Some(installed_pkg) = installed_pkgs.get(dep.name) {
                installed_pkg.version.original == dep.version
            } else {
                false
            }
        });

    let (needs_update, missing): (Vec<_>, Vec<_>) = needs_action.into_iter()
        .partition(|dep| installed_pkgs.contains_key(dep.name));

    info!(
        "Found {} packages installed with correct version, {} need updating, {} missing",
        installed.len(),
        needs_update.len(),
        missing.len()
    );
    for d in &installed {
        debug!("    {d} (already installed)");
    }
    for d in &needs_update {
        info!("    {d} (needs update from: {})", installed_pkgs[d.name].version.original);
    }
    for d in &missing {
        info!("    {d}");
    }
    info!("Plan took: {:?}", total_start_time.elapsed());
}

