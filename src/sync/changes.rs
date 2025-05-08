use std::time::Duration;

use crate::lockfile::Source;
use crate::package::PackageType;
use serde::{Serialize, Serializer};

fn serialize_duration_as_ms<S>(
    duration: &Option<Duration>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match duration {
        Some(duration) => serializer.serialize_u64(duration.as_millis() as u64),
        None => serializer.serialize_none(),
    }
}
#[derive(Debug, Serialize)]
pub struct SyncChange {
    pub name: String,
    #[serde(skip)]
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<PackageType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    #[serde(serialize_with = "serialize_duration_as_ms")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timing: Option<Duration>,
}

impl SyncChange {
    pub fn installed(
        name: &str,
        version: &str,
        source: Source,
        kind: PackageType,
        timing: Duration,
    ) -> Self {
        Self {
            name: name.to_string(),
            installed: true,
            kind: Some(kind),
            timing: Some(timing),
            source: Some(source),
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
                self.source.as_ref().map(|x| x.to_string()).unwrap(),
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
