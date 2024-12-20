use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Cache {
    /// The cache directory.
    /// In practice it will be the OS own cache specific directory + `rv`
    root: PathBuf,
}

impl Cache {
    pub fn new() -> Self {

    }
}