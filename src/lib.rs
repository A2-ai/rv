extern crate core;

mod cache;
mod config;
mod dependency_graph;
mod package;
mod r_cmd;
mod repo_path;
mod repository;
mod resolver;
mod system_info;
mod version;
mod db;
mod useragent;

pub mod cli;
pub mod install;
pub mod consts;

pub use cache::{Cache, CacheEntry};
pub use config::{Config, DependencyKind, Repository};
pub use install::{untar_package, dl_and_install_pkg};
pub use r_cmd::{RCmd, RCommandLine};
pub use repo_path::get_binary_path;
pub use repository::RepositoryDatabase;
pub use resolver::{ResolvedDependency, Resolver};
pub use system_info::{OsType, SystemInfo};
pub use version::Version;
pub use dependency_graph::{BuildPlan, BuildStep};