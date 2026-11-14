use rusqlite::{OptionalExtension, params};
use sage_core::error::{AppError, AppResult};

use crate::domain::{BufferSnapshotAck, BufferSnapshotRequest};

pub(super) fn settled_by_stored(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    request: &BufferSnapshotRequest,
    buffer_fingerprint: &str,
    rebase: bool,
) -> AppResult<(bool, Option<BufferSnapshotAck>)> {
    let stored = transaction
        .query_row(
            "SELECT base_fingerprint, revision, updated_at, buffer_fingerprint
             FROM editor_buffers
             WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3",
            params![
                project_id,
                request.resource.root_id,
                request.resource.relative_path
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((base_fingerprint, revision, updated_at, stored_fingerprint)) = stored else {
        return Ok((false, None));
    };
    if rebase {
        return Ok((true, None));
    }
    if base_fingerprint != request.base.fingerprint {
        return Err(AppError::InvalidRequest(
            "buffer base does not match the durable base".into(),
        ));
    }
    let request_revision = request.revision as i64;
    if request_revision > revision {
        return Ok((true, None));
    }
    if request_revision == revision && stored_fingerprint != buffer_fingerprint {
        return Err(AppError::InvalidRequest(
            "another buffer snapshot already holds this revision".into(),
        ));
    }
    Ok((
        true,
        Some(BufferSnapshotAck {
            revision: revision as u64,
            persisted_at: updated_at as u64,
            dirty: true,
            applied: request_revision == revision,
        }),
    ))
}
