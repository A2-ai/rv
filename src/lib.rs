extern crate core;

mod cache;
mod config;
mod install;
mod package;
mod r_cmd;
mod repo_path;
mod repository;
mod resolver;
mod system_info;
mod version;

#[cfg(feature = "cli")]
pub mod cli;

pub mod consts;

pub use cache::{Cache, CacheEntry};
pub use config::{Config, DependencyKind, Repository};
pub use install::untar_package;
pub use r_cmd::{RCmd, RCommandLine};
pub use repo_path::get_binary_path;
pub use repository::RepositoryDatabase;
pub use resolver::{ResolvedDependency, Resolver};
pub use system_info::{OsType, SystemInfo};
pub use version::Version;
