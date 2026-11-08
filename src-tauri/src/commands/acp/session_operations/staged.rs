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
    let attached_at = attachment(&state.database, project.as_str(), chat.as_str());
    let mut stages = state
        .acp
        .file_stages
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // The prompt was dispatched, so whatever happens to the record, the
    // batch it carried leaves the stage and is never sent twice. A tab that
    // closed meanwhile keeps nothing, and neither does an unreadable one.
    let attached_at = match attached_at {
        Ok(Some(attached_at)) => attached_at,
        Ok(None) => return stages.unstage_sent(project.as_str(), chat.as_str(), files),
        Err(error) => {
            tracing::warn!(?error, "Sent attachments were not kept for Retry");
            return stages.unstage_sent(project.as_str(), chat.as_str(), files);
        }
    };
    stages.consume(
        project.as_str(),
        chat.as_str(),
        attached_at,
        client_intent_id.as_str(),
        files,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::contract::ClientIntentId;

    /// A chat with an open tab and a pasted batch named by the next send.
    fn sent_batch() -> (
        AppState,
        SageProjectId,
        SageChatId,
        Resources,
        std::path::PathBuf,
    ) {
        let root = std::env::temp_dir().join(format!("sage-sent-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap().id;
        let chat = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        let state = AppState::new(database);
        let attached_at = attachment(&state.database, &project, &chat)
            .unwrap()
            .unwrap();
        let mut stages = state.acp.file_stages.lock().unwrap();
        let revision = stages
            .append_text(&project, &chat, attached_at, None, "pasted log".into())
            .unwrap()
            .revision;
        let files = stages
            .selected(&project, &chat, &Selection::Revision(revision))
            .unwrap();
        drop(stages);
        let project = SageProjectId::parse(&project).unwrap();
        let chat = SageChatId::parse(&chat).unwrap();
        (state, project, chat, files, root)
    }

    #[test]
    fn a_dispatched_batch_leaves_the_stage_even_when_its_send_is_not_recorded() {
        let closed_tab = "DELETE FROM chat_tabs";
        let unreadable_tab = "ALTER TABLE chat_tabs RENAME TO chat_tabs_unreadable";
        for breakage in [closed_tab, unreadable_tab] {
            let (state, project, chat, files, root) = sent_batch();
            state.database.connection().execute_batch(breakage).unwrap();
            assert_eq!(
                attachment(&state.database, project.as_str(), chat.as_str()).is_err(),
                breakage == unreadable_tab
            );
            let intent = ClientIntentId::parse("intent-sent").unwrap();
            record_sent(&state, &project, &chat, &intent, &files);
            let stages = state.acp.file_stages.lock().unwrap();
            // The next message does not carry the same files again.
            assert!(
                stages.list(project.as_str(), chat.as_str()).is_none(),
                "{breakage}"
            );
            drop(stages);
            let _ = std::fs::remove_dir_all(root);
        }
    }
}
