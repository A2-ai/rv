use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Serialize, Serializer};

use crate::DiskCache;
use crate::lockfile::Source;
use crate::package::PackageType;
use crate::system_req::{SysDep, SysInstallationStatus};

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sys_deps: Vec<SysDep>,
}

impl SyncChange {
    pub fn installed(
        name: &str,
        version: &str,
        source: Source,
        kind: PackageType,
        timing: Duration,
        sys_deps: Vec<String>,
    ) -> Self {
        Self {
            name: name.to_string(),
            installed: true,
            kind: Some(kind),
            timing: Some(timing),
            source: Some(source),
            version: Some(version.to_string()),
            sys_deps: sys_deps.into_iter().map(SysDep::new).collect(),
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
            sys_deps: Vec::new(),
        }
    }

    pub fn update_sys_deps_status(
        &mut self,
        sysdeps_status: &HashMap<String, SysInstallationStatus>,
    ) {
        for sys_dep in &mut self.sys_deps {
            if let Some(status) = sysdeps_status.get(&sys_dep.name) {
                sys_dep.status = status.clone();
            }
        }
    }

    pub fn print(&self, include_timings: bool, supports_sysdeps_status: bool) -> String {
        if self.installed {
            let sys_deps = {
                let mut out = Vec::new();
                for sys_dep in &self.sys_deps {
                    let status = if !supports_sysdeps_status {
                        String::new()
                    } else {
                        format!(
                            "{} ",
                            if sys_dep.status == SysInstallationStatus::Present {
                                "✓"
                            } else {
                                "✗"
                            }
                        )
                    };
                    out.push(format!("{status}{}", sys_dep.name))
                }
                out
            };
            let mut base = format!(
                "+ {} ({}, {} from {}){}",
                self.name,
                self.version.as_ref().unwrap(),
                self.kind.unwrap(),
                self.source.as_ref().map(|x| x.to_string()).unwrap(),
                if sys_deps.is_empty() {
                    String::new()
                } else {
                    format!(" with sys deps: {}", sys_deps.join(", "))
                }
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

    pub fn is_builtin(&self) -> bool {
        self.source
            .as_ref()
            .map(|x| x == &Source::Builtin { builtin: true })
            .unwrap_or_default()
    }

    pub fn log_path(&self, cache: &DiskCache) -> PathBuf {
        if let Some(s) = &self.source {
            if s.is_repo() {
                cache.get_build_log_path(
                    s,
                    Some(&self.name),
                    Some(&self.version.clone().unwrap().as_str()),
                )
            } else {
                cache.get_build_log_path(s, None, None)
            }
        } else {
            unreachable!("Should not be called with uninstalled deps")
        }
    }
}
