mod activate;
mod add;
mod cache;
mod cancellation;
#[cfg(feature = "cli")]
pub mod cli;
mod config;
mod configure;
mod context;
mod format;
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
pub mod system_req;
mod utils;

pub mod consts;

pub use activate::{activate, deactivate};
pub use add::{AddOptions, add_packages, read_and_verify_config};
pub use cache::{CacheInfo, DiskCache, PackagePaths, utils::hash_string};
pub use cancellation::Cancellation;
pub use config::{Config, ConfigDependency, Repository};
pub use configure::{
    ConfigureRepositoryResponse, RepositoryAction, RepositoryMatcher, RepositoryOperation,
    RepositoryPositioning, RepositoryUpdates, execute_repository_action,
};
pub use context::{Context, RCommandLookup, ResolveMode};
pub use format::format_document;
pub use fs::is_network_fs;
pub use git::{CommandExecutor, GitExecutor, GitRepository};
pub use http::{Http, HttpDownload};
pub use library::Library;
pub use lockfile::{Lockfile, Source};
pub use package::{Dependency, Operator, Version, VersionRequirement, is_binary_package};
pub use project_summary::ProjectSummary;
pub use r_cmd::{RCmd, RCommandLine, find_r_version_command};
pub use renv::RenvLock;
pub use repository::RepositoryDatabase;
pub use repository_urls::{get_package_file_urls, get_tarball_urls};
pub use resolver::{Resolution, ResolvedDependency, Resolver, UnresolvedDependency};
pub use sync::{BuildPlan, BuildStep, LinkMode, SyncChange, SyncHandler};
pub use system_info::{OsType, SystemInfo};

#[doc(hidden)]
pub mod internal {
    pub use crate::package::parse_dependencies;
}
