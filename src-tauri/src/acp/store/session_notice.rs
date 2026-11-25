//! A session notice that no Turn owns, such as why the runtime stopped
//! before the session's first Turn. It folds into the session's bounded
//! surface row like a harness notice, for the live runtime epoch only.

use sage_acp_protocol::{
    contract::{ProjectionRevision, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    intent::session_event::{SessionNotice, SessionScopedEvent},
};

use super::{StoreError, session_surface, turn_admission::require_current_runtime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionNoticeCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub notice: SessionNotice,
    pub now_millis: i64,
}

/// The projection revision the notice was published at, or zero when the
/// same notice was already shown.
pub(crate) fn record_session_notice(
    connection: &mut rusqlite::Connection,
    command: &SessionNoticeCommand,
) -> Result<ProjectionRevision, StoreError> {
    let transaction = connection.transaction()?;
    let owner = [
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
    ];
    require_current_runtime(
        &transaction,
        owner[0],
        owner[1],
        owner[2],
        command.runtime_epoch,
    )?;
    let published = session_surface::notice_in(
        &transaction,
        owner,
        command.runtime_epoch.get(),
        &session_surface::SessionEventCommand {
            event: SessionScopedEvent::Notice(command.notice.clone()),
            now_millis: command.now_millis,
        },
    )?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(published.unwrap_or(ProjectionRevision::ZERO))
}
