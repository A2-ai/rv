use std::fmt;
use std::fmt::Formatter;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum InstallationStatus {
    Absent,
    Source,
    /// The bool represents whether it has been built from source by rv
    Binary(bool),
    /// The bool represents whether the binary has been built from source by rv
    Both(bool),
}

impl InstallationStatus {
    pub fn available(&self) -> bool {
        *self != InstallationStatus::Absent
    }

    pub fn binary_available(&self) -> bool {
        matches!(
            self,
            InstallationStatus::Binary(_) | InstallationStatus::Both(_)
        )
    }

    pub fn binary_available_from_source(&self) -> bool {
        matches!(
            self,
            InstallationStatus::Binary(true) | InstallationStatus::Both(true)
        )
    }

    /// If the user asked force_source and we have binary version but not built from source ourselves,
    /// consider we don't actually have the binary
    pub fn mark_as_binary_unavailable(self) -> Self {
        match self {
            InstallationStatus::Both(false) => InstallationStatus::Source,
            InstallationStatus::Binary(false) => InstallationStatus::Absent,
            _ => self,
        }
    }

    pub fn source_available(&self) -> bool {
        matches!(
            self,
            InstallationStatus::Source | InstallationStatus::Both(_)
        )
    }
}

impl fmt::Display for InstallationStatus {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            InstallationStatus::Source => write!(f, "source"),
            InstallationStatus::Binary(b) => write!(f, "binary (built from source: {b})"),
            InstallationStatus::Both(b) => write!(f, "source and binary (built from source: {b})"),
            InstallationStatus::Absent => write!(f, "absent"),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct CacheStatus {
    pub local: InstallationStatus,
    pub global: Option<InstallationStatus>,
}

impl CacheStatus {
    pub fn new_local_source() -> Self {
        Self {
            local: InstallationStatus::Source,
            global: None,
        }
    }

    pub fn new_local_builtin_binary() -> Self {
        Self {
            local: InstallationStatus::Binary(false),
            global: None,
        }
    }

    pub fn mark_as_binary_unavailable(self) -> Self {
        Self {
            local: self.local.mark_as_binary_unavailable(),
            global: self.global.map(|g| g.mark_as_binary_unavailable()),
        }
    }

    pub fn local_binary_available(&self) -> bool {
        self.local.binary_available()
    }

    pub fn global_binary_available(&self) -> bool {
        self.global.map(|x| x.binary_available()).unwrap_or(false)
    }

    pub fn binary_available(&self) -> bool {
        self.local.binary_available() || self.global_binary_available()
    }

    pub fn source_available(&self) -> bool {
        self.local.source_available() || self.global.map(|x| x.source_available()).unwrap_or(false)
    }
}
