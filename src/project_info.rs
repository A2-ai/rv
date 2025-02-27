use std::fmt;

use serde::Serialize;

use crate::Version;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectInfo<'a> {
    r_version: &'a Version,
}

impl<'a> ProjectInfo<'a> {
    pub fn new(r_version: &'a Version) -> Self {
        Self { r_version }
    }
}

impl fmt::Display for ProjectInfo<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "R Version: {}", self.r_version)?;
        Ok(())
    }
}

