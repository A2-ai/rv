//! This crate provides a library for installing and managing R packages
#![warn(missing_docs)]
mod activate;
mod add;
mod cache;
mod config;
mod fs;
mod git;
mod http;
mod library;
mod lockfile;
mod package;
mod project_summary;
mod r_cmd;
mod renv;
mod repository;
mod repository_urls;
mod resolver;
mod sync;
mod system_info;

#[cfg(feature = "cli")]
/// CLI commands for the library
pub mod cli;

/// Constants used in the library
pub mod consts;

pub use activate::{activate, deactivate};
pub use add::{add_packages, read_and_verify_config};
pub use cache::{utils::hash_string, CacheInfo, DiskCache, InstallationStatus, PackagePaths};
pub use config::{Config, ConfigDependency, Repository};
pub use git::{CommandExecutor, GitExecutor, GitRepository};
pub use http::{Http, HttpDownload};
pub use library::Library;
pub use lockfile::Lockfile;
pub use package::{
    is_binary_package, Package, Version, VersionRequirement,
};
pub use project_summary::ProjectSummary;
pub use r_cmd::{
    find_r_version_command, BuildError, BuildErrorKind, CheckError, CheckErrorKind, InstallError,
    InstallErrorKind, RCmd, RCommandLine, VersionError, VersionErrorKind,
};
pub use renv::RenvLock;
pub use repository::RepositoryDatabase;
pub use repository_urls::{get_package_file_urls, get_tarball_urls};
pub use resolver::{ResolvedDependency, Resolver, UnresolvedDependency};
pub use sync::{BuildPlan, BuildStep, LinkMode, SyncHandler};
pub use system_info::{OsType, SystemInfo};

#[doc(hidden)]
pub mod internal {
    pub use crate::package::{parse_dependencies, parse_description_file, parse_package_file, Dependency, Operator,};
    pub use crate::repository_urls::get_distro_name;
    pub use crate::sync::LinkMode;

}