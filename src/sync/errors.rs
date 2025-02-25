use std::io;

use crate::http::HttpError;
use crate::r_cmd::InstallError;
use crate::sync::LinkError;

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
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error("Failed to link files from cache: {0:?})")]
    LinkError(LinkError),
    #[error("Failed to install package: {0:?})")]
    InstallError(InstallError),
    #[error("Failed to download package: {0:?})")]
    HttpError(HttpError),
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

impl From<git2::Error> for SyncError {
    fn from(error: git2::Error) -> Self {
        Self {
            source: SyncErrorKind::Git(error),
        }
    }
}
