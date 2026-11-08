//! Which staged prompt files a send carries, resolved against durable state.
use super::*;
use crate::acp::attached_context::files::{Resources, Selection, attachment, retry_origin};

/// A Retry names a turn; its durable record says what the prompt carried.
pub(super) fn resolve(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
    staged: Option<crate::acp::intent::StagedSelection>,
) -> Result<Selection, AcpIpcError> {
    use crate::acp::intent::StagedSelection;
    Ok(match staged {
        None => Selection::None,
        Some(StagedSelection::Revision(revision)) => Selection::Revision(revision),
        Some(StagedSelection::RetryOf(turn)) => Selection::Retry(
            retry_origin(
                &state.database,
                project.as_str(),
                chat.as_str(),
                turn.as_str(),
            )
            .map_err(|_| internal_error())?
            .ok_or_else(|| AcpIpcError::InvalidRequest {
                message: safe_message("That message is no longer in this chat."),
            })?,
        ),
    })
}

/// The characterization harness takes no files: a send that names some is
/// refused, and a stale selection keeps its own message.
#[cfg(all(windows, feature = "acp-fixture"))]
pub(super) fn characterization_carries_none(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
    selection: &Selection,
) -> Result<(), AcpIpcError> {
    let files = state
        .acp
        .file_stages
        .lock()
        .map_err(|_| internal_error())?
        .selected(project.as_str(), chat.as_str(), selection)
        .map_err(|message| AcpIpcError::InvalidRequest {
            message: safe_message(message),
        })?;
    if files.0.is_empty() {
        return Ok(());
    }
    Err(AcpIpcError::InvalidRequest {
        message: safe_message("This harness cannot accept staged file attachments."),
    })
}

/// Records what a dispatched prompt carried, under its client intent.
pub(super) fn record_sent(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
    client_intent_id: &crate::acp::contract::ClientIntentId,
    files: &Resources,
) {
    // A tab that closed meanwhile keeps nothing; the sweep drops its batch.
    let Ok(Some(attached_at)) = attachment(&state.database, project.as_str(), chat.as_str()) else {
        return;
    };
    state
        .acp
        .file_stages
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .consume(
            project.as_str(),
            chat.as_str(),
            attached_at,
            client_intent_id.as_str(),
            files,
        );
}
