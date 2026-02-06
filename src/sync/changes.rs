use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Serialize, Serializer};

use serde::Deserialize;

use crate::DiskCache;
use crate::lockfile::Source;
use crate::package::PackageType;
use crate::system_req::{SysDep, SysInstallationStatus};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheSource {
    Global,
    Local,
}

/// Sections for grouping sync output
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OutputSection {
    GlobalCache,
    LocalCache,
    Downloaded,
    LocalPath,
    Removed,
}

impl OutputSection {
    pub fn header(&self) -> &'static str {
        match self {
            Self::GlobalCache => "From global cache",
            Self::LocalCache => "From local cache",
            Self::Downloaded => "Downloaded",
            Self::LocalPath => "From local path",
            Self::Removed => "Removed",
        }
    }
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_source: Option<CacheSource>,
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
        cache_source: Option<CacheSource>,
    ) -> Self {
        Self {
            name: name.to_string(),
            installed: true,
            kind: Some(kind),
            timing: Some(timing),
            source: Some(source),
            cache_source,
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
            cache_source: None,
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
            let cache_desc = match self.cache_source {
                Some(CacheSource::Global) => "found in global cache",
                Some(CacheSource::Local) => "found in cache",
                None => "downloaded",
            };
            let sys_deps_string = if sys_deps.is_empty() {
                String::new()
            } else {
                format!(" with sys deps: {}", sys_deps.join(", "))
            };
            let mut base = format!(
                "+ {} ({}): {} {}, from {}{}",
                self.name,
                self.version.as_ref().unwrap(),
                self.kind.unwrap(),
                cache_desc,
                self.source.as_ref().map(|x| x.to_string()).unwrap(),
                sys_deps_string
            );

            if include_timings {
                base += &format!(" ({}ms)", self.timing.unwrap().as_millis());
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

    /// Determine which output section this change belongs to
    pub fn section(&self) -> OutputSection {
        if !self.installed {
            return OutputSection::Removed;
        }
        if matches!(self.source, Some(Source::Local { .. })) {
            return OutputSection::LocalPath;
        }
        match self.cache_source {
            Some(CacheSource::Global) => OutputSection::GlobalCache,
            Some(CacheSource::Local) => OutputSection::LocalCache,
            None => OutputSection::Downloaded,
        }
    }

    /// Extract URL or path for display in output
    pub fn source_display(&self) -> String {
        match &self.source {
            Some(Source::Repository { repository }) => repository.to_string(),
            Some(Source::Git { git, .. }) | Some(Source::RUniverse { git, .. }) => {
                git.url().to_string()
            }
            Some(Source::Url { url, .. }) => url.to_string(),
            Some(Source::Local { path, .. }) => path.display().to_string(),
            Some(Source::Builtin { .. }) | None => String::new(),
        }
    }

    pub fn log_path(&self, cache: &DiskCache) -> PathBuf {
        if let Some(s) = &self.source {
            if s.is_repo() {
                cache.get_build_log_path(
                    s,
                    Some(&self.name),
                    Some(self.version.clone().unwrap().as_str()),
                )
            } else {
                cache.get_build_log_path(s, None, None)
            }
        } else {
            unreachable!("Should not be called with uninstalled deps")
        }
    }
}
