extern crate core;

mod build_plan;
mod cache;
mod config;
mod fs;
mod lockfile;
mod package;
mod r_cmd;
mod renv_lock;
mod renv_resolve;
mod repo_path;
mod repository;
mod resolver;
mod system_info;
mod version;

#[cfg(feature = "cli")]
pub mod cli;

pub mod consts;

pub use build_plan::{BuildPlan, BuildStep};
pub use cache::{Cache, CacheEntry};
pub use config::{Config, DependencyKind, Repository};
pub use lockfile::Lockfile;
pub use r_cmd::{RCmd, RCommandLine};
pub use repo_path::RepoServer;
pub use repository::RepositoryDatabase;
pub use resolver::{ResolutionNeeded, ResolvedDependency, Resolver, UnresolvedDependency};
pub use system_info::{OsType, SystemInfo};
pub use version::Version;
