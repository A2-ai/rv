use crate::package::PackageType;
use std::time::Duration;

#[derive(Debug)]
pub struct SyncChange {
    pub name: String,
    pub installed: bool,
    pub kind: Option<PackageType>,
    pub version: Option<String>,
    pub source: Option<String>,
    pub timing: Option<Duration>,
}

impl SyncChange {
    pub fn installed(
        name: &str,
        version: &str,
        source: &str,
        kind: PackageType,
        timing: Duration,
    ) -> Self {
        Self {
            name: name.to_string(),
            installed: true,
            kind: Some(kind),
            timing: Some(timing),
            source: Some(source.to_string()),
            version: Some(version.to_string()),
        }
    }

    pub fn removed(name: &str) -> Self {
        Self {
            name: name.to_string(),
            installed: false,
            kind: None,
            timing: None,
            source: None,
            version: None,
        }
    }

    pub fn print(&self, include_timings: bool) -> String {
        if self.installed {
            let mut base = format!(
                "+ {} ({}, {} from {})",
                self.name,
                self.version.as_ref().unwrap(),
                self.kind.unwrap(),
                self.source.as_ref().unwrap(),
            );

            if include_timings {
                base += &format!(" in {}ms", self.timing.unwrap().as_millis());
                base
            } else {
                base
            }
        } else {
            format!("- {}", self.name)
        }
    }
}
