mod activate;
mod add_remove;
mod cache;
mod config;
mod fs;
mod git;
mod http;
mod library;
mod lockfile;
mod package;
mod project_info;
mod r_cmd;
mod renv;
mod repository;
mod repository_urls;
mod resolver;
mod sync;
mod system_info;

#[cfg(feature = "cli")]
pub mod cli;

pub mod consts;

pub use activate::{activate, deactivate};
pub use add_remove::{add_packages, read_and_verify_config, remove_packages};
pub use cache::{utils::hash_string, CacheInfo, DiskCache, PackagePaths};
pub use config::{Config, ConfigDependency, Repository};
pub use git::{Git, GitOperations};
pub use http::{Http, HttpDownload};
pub use library::Library;
pub use lockfile::Lockfile;
pub use package::{is_binary_package, Version, VersionRequirement};
pub use project_info::ProjectInfo;
pub use r_cmd::{find_r_version_command, RCmd, RCommandLine};
pub use renv::RenvLock;
pub use repository::RepositoryDatabase;
pub use repository_urls::{get_package_file_urls, get_tarball_urls};
pub use resolver::{ResolvedDependency, Resolver, UnresolvedDependency};
pub use sync::{BuildPlan, BuildStep, SyncHandler};
pub use system_info::{OsType, SystemInfo};
