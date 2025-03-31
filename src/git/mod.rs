use std::process::Command;

mod local;
mod reference;
mod remote;

/// A trait for executing cli commands
pub trait CommandExecutor {
    /// execute a given command
    fn execute(&self, command: &mut Command) -> Result<String, std::io::Error>;
}

pub use local::GitRepository;
pub use reference::GitReference;
pub use remote::GitRemote;

#[derive(Debug, Clone)]
/// A simple struct to implement the CommandExecutor trait for Git
pub struct GitExecutor;

impl CommandExecutor for GitExecutor {
    fn execute(&self, command: &mut Command) -> Result<String, std::io::Error> {
        let res = command.output()?;
        if res.status.success() {
            Ok(String::from_utf8_lossy(&res.stdout).trim().to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                String::from_utf8_lossy(&res.stderr),
            ))
        }
    }
}
