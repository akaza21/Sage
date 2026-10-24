use std::sync::Mutex;

use crate::acp::{contract::ProjectionRevision, intent::ProjectionChangedV1};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum MailboxError {
    #[error("projection notification revision must be positive")]
    InvalidRevision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishDisposition {
    Published,
    Coalesced,
    BootstrapRequired,
}

#[derive(Debug, Default)]
pub(crate) struct ProjectionMailbox {
    pending: Mutex<Option<ProjectionChangedV1>>,
}

impl ProjectionMailbox {
    pub(crate) fn publish_committed(
        &self,
        event: ProjectionChangedV1,
    ) -> Result<PublishDisposition, MailboxError> {
        if event.projection_revision == ProjectionRevision::ZERO {
            return Err(MailboxError::InvalidRevision);
        }
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match pending.as_mut() {
            None => {
                *pending = Some(event);
                Ok(PublishDisposition::Published)
            }
            Some(current)
                if current.project_id == event.project_id
                    && current.chat_id == event.chat_id
                    && current.session_id == event.session_id =>
            {
                if (event.projection_revision, event.child_projection_epoch)
                    > (current.projection_revision, current.child_projection_epoch)
                {
                    *current = event;
                }
                Ok(PublishDisposition::Coalesced)
            }
            Some(_) => Ok(PublishDisposition::BootstrapRequired),
        }
    }

    pub(crate) fn take(&self) -> Option<ProjectionChangedV1> {
        self.pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}
