use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum PreviewError {
    #[error("{message}")]
    InvalidRequest { message: String },
    #[error("{message}")]
    Conflict {
        resource: &'static str,
        message: String,
    },
    #[error("{message}")]
    Unavailable { message: String },
    #[error("{message}")]
    Internal { message: String },
}

impl PreviewError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidRequest {
            message: message.into(),
        }
    }

    pub(crate) fn conflict(resource: &'static str, message: impl Into<String>) -> Self {
        Self::Conflict {
            resource,
            message: message.into(),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
}
