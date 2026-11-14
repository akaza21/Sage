use crate::acp::ipc::ProjectionChangedV1;

#[cfg(test)]
use super::{MailboxError, ProjectionMailbox, PublishDisposition};
use super::{NotificationRecovery, ProjectionNotifier, PublishError, PublishOutcome};

/// Whether a commit may have admitted a native turn notice receipt: a notified
/// Turn outcome or a permission request. Streamed content never does, so its
/// publications skip the notice observer and its database work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnNotice {
    None,
    Possible,
}

impl TurnNotice {
    pub(crate) fn possible_if(condition: bool) -> Self {
        if condition {
            Self::Possible
        } else {
            Self::None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectionPublishStatus {
    Published,
    Coalesced,
    BootstrapRequired,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ProjectionPublishError {
    #[error("projection notification revision must be positive")]
    InvalidRevision,
}

pub(crate) trait ProjectionPublisher: Send + Sync {
    fn publish_committed(
        &self,
        event: ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError>;

    /// Publishes a commit, saying whether it may have admitted a native turn
    /// notice. Only `TurnNotice::Possible` reaches the notice observer.
    fn publish_committed_with(
        &self,
        event: ProjectionChangedV1,
        _turn_notice: TurnNotice,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        self.publish_committed(event)
    }
}

#[cfg(test)]
impl ProjectionPublisher for ProjectionMailbox {
    fn publish_committed(
        &self,
        event: ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        ProjectionMailbox::publish_committed(self, event)
            .map(mailbox_status)
            .map_err(mailbox_error)
    }
}

impl ProjectionPublisher for ProjectionNotifier {
    fn publish_committed(
        &self,
        event: ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        ProjectionNotifier::publish_committed(self, event)
            .map(notifier_status)
            .map_err(notifier_error)
    }

    fn publish_committed_with(
        &self,
        event: ProjectionChangedV1,
        turn_notice: TurnNotice,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        ProjectionNotifier::publish_committed_with(self, event, turn_notice)
            .map(notifier_status)
            .map_err(notifier_error)
    }
}

#[cfg(test)]
fn mailbox_status(status: PublishDisposition) -> ProjectionPublishStatus {
    match status {
        PublishDisposition::Published => ProjectionPublishStatus::Published,
        PublishDisposition::Coalesced => ProjectionPublishStatus::Coalesced,
        PublishDisposition::BootstrapRequired => ProjectionPublishStatus::BootstrapRequired,
    }
}

fn notifier_status(status: PublishOutcome) -> ProjectionPublishStatus {
    match status {
        PublishOutcome::Scheduled => ProjectionPublishStatus::Published,
        PublishOutcome::Coalesced | PublishOutcome::SuppressedInactive => {
            ProjectionPublishStatus::Coalesced
        }
        PublishOutcome::BootstrapRequired => ProjectionPublishStatus::BootstrapRequired,
        PublishOutcome::RecoveryRequired(
            NotificationRecovery::TupleContention | NotificationRecovery::SinkFailure,
        )
        | PublishOutcome::Shutdown => ProjectionPublishStatus::RecoveryRequired,
    }
}

#[cfg(test)]
fn mailbox_error(_: MailboxError) -> ProjectionPublishError {
    ProjectionPublishError::InvalidRevision
}

fn notifier_error(_: PublishError) -> ProjectionPublishError {
    ProjectionPublishError::InvalidRevision
}
