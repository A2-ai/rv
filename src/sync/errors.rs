use crate::http::HttpError;
use crate::r_cmd::InstallError;
use crate::sync::LinkError;
use std::fmt;
use std::fmt::Formatter;
use std::io;

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
#[non_exhaustive]
pub struct SyncError {
    pub source: SyncErrorKind,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncErrorKind {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("Failed to link files from cache: {0:?})")]
    LinkError(LinkError),
    #[error("Failed to install R package: {0:?})")]
    InstallError(InstallError),
    #[error("Failed to download package: {0:?})")]
    HttpError(HttpError),
    #[error("{0}")]
    SyncFailed(SyncErrors),
}

impl From<InstallError> for SyncError {
    fn from(error: InstallError) -> Self {
        Self {
            source: SyncErrorKind::InstallError(error),
        }
    }
}

impl From<LinkError> for SyncError {
    fn from(error: LinkError) -> Self {
        Self {
            source: SyncErrorKind::LinkError(error),
        }
    }
}

impl From<HttpError> for SyncError {
    fn from(error: HttpError) -> Self {
        Self {
            source: SyncErrorKind::HttpError(error),
        }
    }
}

impl From<io::Error> for SyncError {
    fn from(error: io::Error) -> Self {
        Self {
            source: SyncErrorKind::Io(error),
        }
    }
}

#[derive(Debug)]
pub struct SyncErrors {
    pub(crate) errors: Vec<(String, SyncError)>,
}

impl fmt::Display for SyncErrors {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to install dependencies.")?;

        for (dep, e) in &self.errors {
            write!(f, "\n    Failed to install {dep}:\n        {e}")?;
        }

        Ok(())
    }
}
