use std::str::FromStr;
use clap::ValueEnum;

use crate::{
    cli::DiskCache, db::load_databases, Config, RCommandLine, Resolver, SystemInfo, Version
    // anything else needed...
};
use log::{trace, debug, info, error};

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
}

pub fn execute_plan(
    config: &Config,
    plan_args: PlanArgs,
) {
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
            SystemInfo::new(
                crate::OsType::MacOs,
                Some("aarch64".into()),
                None,
                "12.0",
            )
        },
        Some(Distribution::Windows) => {
            // Example for Windows
            SystemInfo::new(
                crate::OsType::Windows,
                Some("x86_64".into()),
                None,
                "12.0",
            )
        },
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
        },
        None => SystemInfo::from_os_info(),
    };
    trace!("time to get sysinfo: {:?}", start_time.elapsed());

    // Load databases
    start_time = std::time::Instant::now();
    let cache = DiskCache::new(&r_version, sysinfo.clone());
    let databases = load_databases(
        config.repositories(),
        &cache,
        &r_version,
        no_user_override,
    );
    debug!("Loading databases took: {:?}", start_time.elapsed());

    // Resolve
    start_time = std::time::Instant::now();
    let resolver = Resolver::new(&databases, &r_version);
    let (resolved, unresolved) = resolver.resolve(config.dependencies());
    trace!("Resolving took: {:?}", start_time.elapsed());

    if unresolved.is_empty() {
        info!("Plan successful! The following packages will be installed:");
        for d in resolved {
            info!("    {d}");
        }
    } else {
        error!("Failed to find all dependencies");
        for d in unresolved {
            info!("    {d}");
        }
    }
    info!("Plan took: {:?}", total_start_time.elapsed());
}