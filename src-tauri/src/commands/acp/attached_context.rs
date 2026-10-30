use super::*;

pub(super) fn validate_attached_context(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    expected_revision: crate::acp::contract::ProjectionRevision,
    contexts: &AttachedContexts,
) -> Result<(), AcpIpcError> {
    if contexts.as_slice().iter().all(|context| {
        matches!(
            context,
            crate::acp::attached_context::AttachedContext::BrowserPage(_)
                | crate::acp::attached_context::AttachedContext::BrowserElement(_)
        )
    }) {
        return Ok(());
    }
    let connection = state.database.connection();
    crate::acp::transcript_quote::validate_quote_sources(&connection, project_id, contexts)?;
    let mut statement = connection
        .prepare("SELECT id FROM project_roots WHERE project_id=?1 ORDER BY ordinal,id LIMIT 9")
        .map_err(|_| internal_error())?;
    let roots = statement
        .query_map([project_id.as_str()], |row| row.get::<_, String>(0))
        .map_err(|_| internal_error())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| internal_error())?;
    if roots.is_empty() || roots.len() > 8 {
        return Err(internal_error());
    }
    let roots = roots
        .into_iter()
        .map(|root| crate::acp::contract::SageRootId::parse(root).map_err(|_| internal_error()))
        .collect::<Result<Vec<_>, _>>()?;
    for context in contexts.as_slice() {
        let crate::acp::attached_context::AttachedContext::DiffLineComment(context) = context
        else {
            continue;
        };
        let reference = &context.diff_ref;
        if reference.project_id != project_id.as_str() || reference.chat_id != chat_id.as_str() {
            return Err(AcpIpcError::InvalidRequest {
                message: safe_message("Attached context belongs to a different project or Chat."),
            });
        }
        let snapshot =
            crate::acp::service::verify_current_owner(&connection, expected_revision, reference)
                .and_then(|()| read_snapshot(&connection, reference, &roots))
                .map_err(|error| match error {
                    crate::acp::file_diff::SnapshotReadError::Stale
                    | crate::acp::file_diff::SnapshotReadError::Unavailable => {
                        AcpIpcError::Conflict {
                            resource: crate::acp::intent::AcpConflictResource::Session,
                            message: safe_message(
                                "Attached context belongs to an unavailable file snapshot.",
                            ),
                        }
                    }
                    crate::acp::file_diff::SnapshotReadError::Forbidden => {
                        AcpIpcError::InvalidRequest {
                            message: safe_message(
                                "Attached context is not available to the current owner.",
                            ),
                        }
                    }
                    crate::acp::file_diff::SnapshotReadError::Corrupt => {
                        AcpIpcError::RecoveryRequired {
                            message: safe_message("Attached context failed its integrity check."),
                        }
                    }
                })?;
        context
            .validate_line_exists(&snapshot.old_text, &snapshot.new_text)
            .map_err(|_| AcpIpcError::InvalidRequest {
                message: safe_message("Attached context line range is outside the saved snapshot."),
            })?;
    }
    Ok(())
}
