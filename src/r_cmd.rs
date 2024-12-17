use std::{process::{Command, Output}, path::Path, fs};

trait RCmd {
    fn install(&self, library: &Path, args: Option<Vec<&str>>, env_var: Option<Vec<(&str, &str)>>) -> Result<Output, std::io::Error>;
    fn check(&self, result_path: &Path, args: Option<Vec<&str>>, env_var: Option<Vec<(&str, &str)>>) -> Result<Output, std::io::Error>;
    fn build(&self, library: &Path, output_path: &Path, args: Option<Vec<&str>>, env_var: Option<Vec<(&str, &str)>>) -> Result<Output, std::io::Error>;
}