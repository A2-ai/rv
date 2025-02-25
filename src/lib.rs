mod activate;
mod cache;
mod config;
mod fs;
mod git;
mod http;
mod library;
mod lockfile;
mod package;
mod r_cmd;
mod renv;
mod repo_path;
mod repository;
mod resolver;
mod sync;
mod system_info;

#[cfg(feature = "cli")]
pub mod cli;

pub mod consts;

pub use activate::{activate, deactivate};
pub use cache::{utils::hash_string, DiskCache, PackagePaths};
pub use config::{Config, ConfigDependency, Repository};
pub use git::{Git, GitOperations};
pub use http::{Http, HttpDownload};
pub use library::Library;
pub use lockfile::Lockfile;
pub use package::{is_binary_package, Version, VersionRequirement};
pub use r_cmd::{find_r_version_command, RCmd, RCommandLine};
pub use renv::RenvLock;
pub use repo_path::RepoServer;
pub use repository::RepositoryDatabase;
pub use resolver::{ResolvedDependency, Resolver, UnresolvedDependency};
pub use sync::{BuildPlan, BuildStep};
pub use system_info::{OsType, SystemInfo};
