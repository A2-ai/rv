extern crate core;

mod config;
mod http;
mod package;
mod r_cmd;
mod repository;
mod resolver;
mod version;

#[cfg(feature = "cli")]
pub mod cli;

pub use config::{Config, DependencyKind, Repository};
pub use r_cmd::{RCmd, RCommandLine};
pub use repository::RepositoryDatabase;
pub use resolver::{ResolvedDependency, Resolver};
