use super::*;

#[tauri::command]
pub async fn get_acp_file_snapshot(
    state: State<'_, AppState>,
    request: GetAcpFileSnapshotRequest,
) -> Result<AcpFileSnapshotResponse, AcpIpcError> {
    get_acp_file_snapshot_inner(&state, request).await
}

#[cfg(test)]
mod tests;

pub(crate) async fn get_acp_file_snapshot_inner(
    state: &AppState,
    request: GetAcpFileSnapshotRequest,
) -> Result<AcpFileSnapshotResponse, AcpIpcError> {
    let page_read = state
        .acp
        .reserve_page_read(&request.project_id, &request.chat_id)
        .map_err(page_admission_error)?;
    let _transition =
        lock_acp_context(state, &request.project_id, request.project_generation).await?;
    if request.diff_ordinal != crate::acp::file_diff::DIFF_ORDINAL
        || request.snapshot_revision == 0
        || request.relative_path.is_empty()
        || request.relative_path.len() > 16_384
        || request.content_digest.len() != 64
    {
        return Err(snapshot_error(
            crate::acp::file_diff::SnapshotReadError::Forbidden,
        ));
    }
    let diff_ref = crate::acp::file_diff::DiffRef {
        project_id: request.project_id.as_str().to_owned(),
        chat_id: request.chat_id.as_str().to_owned(),
        session_id: request.session_id.as_str().to_owned(),
        runtime_epoch: request.runtime_epoch.get(),
        turn_id: request.turn_id.as_str().to_owned(),
        item_id: request.item_id.as_str().to_owned(),
        diff_ordinal: request.diff_ordinal,
        snapshot_revision: request.snapshot_revision,
        root_id: request.root_id.as_str().to_owned(),
        relative_path: request.relative_path.clone(),
        content_digest: request.content_digest.clone(),
    };
    let data = state
        .acp
        .read_file_snapshot(
            &request.project_id,
            request.expected_projection_revision,
            diff_ref,
            page_read,
        )
        .await
        .map_err(snapshot_error)?;
    if data.old_text.len() != data.old_bytes || data.new_text.len() != data.new_bytes {
        return Err(snapshot_error(
            crate::acp::file_diff::SnapshotReadError::Corrupt,
        ));
    }
    Ok(AcpFileSnapshotResponse {
        project_id: request.project_id,
        projection_revision: request.expected_projection_revision,
        chat_id: request.chat_id,
        session_id: request.session_id,
        runtime_epoch: request.runtime_epoch,
        turn_id: request.turn_id,
        item_id: request.item_id,
        diff_ordinal: request.diff_ordinal,
        root_id: request.root_id,
        relative_path: request.relative_path,
        snapshot_revision: data.snapshot_revision,
        old_text: data.old_text,
        new_text: data.new_text,
        old_bytes: data.old_bytes as u64,
        new_bytes: data.new_bytes as u64,
        content_digest: data.content_digest,
    })
}

fn snapshot_error(error: crate::acp::file_diff::SnapshotReadError) -> AcpIpcError {
    use crate::acp::file_diff::SnapshotReadError;
    match error {
        SnapshotReadError::Unavailable => AcpIpcError::Unavailable {
            reason: crate::acp::intent::AcpUnavailableReason::SessionUnavailable,
            message: safe_message(error.message()),
        },
        SnapshotReadError::Stale => AcpIpcError::Conflict {
            resource: crate::acp::intent::AcpConflictResource::Session,
            message: safe_message(error.message()),
        },
        SnapshotReadError::Corrupt => AcpIpcError::RecoveryRequired {
            message: safe_message(error.message()),
        },
        SnapshotReadError::Forbidden => AcpIpcError::InvalidRequest {
            message: safe_message(error.message()),
        },
    }
}
