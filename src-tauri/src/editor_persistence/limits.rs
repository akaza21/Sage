use rusqlite::params;

use crate::error::{AppError, AppResult};

pub const MAX_DIRTY_BUFFERS: i64 = 16;
pub const MAX_TOTAL_DIRTY_BYTES: i64 = 8 * 1024 * 1024;
pub const MAX_TOTAL_DIRTY_METADATA_BYTES: i64 = 1024 * 1024;

pub(super) fn preflight_dirty_recovery(
    connection: &rusqlite::Connection,
    project_id: &str,
) -> AppResult<()> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM editor_buffers
           WHERE project_id = ?1 LIMIT ?2
         )",
        params![project_id, MAX_DIRTY_BUFFERS + 1],
        |row| row.get(0),
    )?;
    if count > MAX_DIRTY_BUFFERS {
        return Err(AppError::ResourceLimit(format!(
            "project exceeds its {MAX_DIRTY_BUFFERS}-buffer recovery limit"
        )));
    }
    let foreign_root: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM editor_buffers b
           LEFT JOIN project_roots r
             ON r.id = b.root_id AND r.project_id = b.project_id
           WHERE b.project_id = ?1 AND r.id IS NULL
         )",
        [project_id],
        |row| row.get(0),
    )?;
    if foreign_root {
        return Err(AppError::InvalidRequest(
            "dirty buffer references a foreign project root".into(),
        ));
    }
    let (content_bytes, metadata_bytes): (i64, i64) = connection.query_row(
        "SELECT COALESCE(SUM(length(CAST(buffer_content AS BLOB))), 0),
                COALESCE(SUM(
           length(CAST(root_id AS BLOB))
           + length(CAST(relative_path AS BLOB))
           + length(CAST(base_fingerprint AS BLOB))
           + length(CAST(line_ending AS BLOB))
           + length(CAST(buffer_fingerprint AS BLOB))
           + COALESCE(length(CAST(pending_temp_path AS BLOB)), 0)
         ), 0)
         FROM editor_buffers WHERE project_id = ?1",
        [project_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if content_bytes > MAX_TOTAL_DIRTY_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "dirty-buffer recovery exceeds the {} MiB project limit",
            MAX_TOTAL_DIRTY_BYTES / 1024 / 1024
        )));
    }
    if metadata_bytes > MAX_TOTAL_DIRTY_METADATA_BYTES {
        return Err(AppError::ResourceLimit(
            "dirty-buffer recovery metadata exceeds its 1 MiB project limit".into(),
        ));
    }
    Ok(())
}
