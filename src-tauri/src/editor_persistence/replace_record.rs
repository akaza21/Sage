//! The recovery record a workspace Replace keeps while it swaps a File.

use rusqlite::params;

use super::{PersistedEditorBuffer, limits::preflight_dirty_recovery};
use crate::{
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
};

impl Database {
    /// Records a workspace Replace about to swap `buffer.resource` on disk,
    /// as a save records its pending temporary file: a recovery buffer that
    /// holds the replaced text and names the temporary file. Startup
    /// recovery removes the temporary file and, once the File on disk is the
    /// replaced text, the buffer; if the swap never happened, the replaced
    /// text stays as unsaved edits. Refused when the File already has a
    /// buffer, or when one more would break the recovery limits.
    pub fn record_replace_in_progress(
        &self,
        project_id: &str,
        buffer: &PersistedEditorBuffer,
    ) -> AppResult<()> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let inserted = transaction.execute(
            "INSERT INTO editor_buffers (
               project_id, root_id, relative_path, base_fingerprint,
               base_byte_len, base_modified_at, line_ending, buffer_content,
               buffer_byte_len, buffer_fingerprint, revision,
               pending_temp_path, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12)
             ON CONFLICT(project_id, root_id, relative_path) DO NOTHING",
            params![
                project_id,
                buffer.resource.root_id,
                buffer.resource.relative_path,
                buffer.base_fingerprint,
                buffer.base_byte_len as i64,
                buffer.base_modified_at.map(|value| value as i64),
                buffer.line_ending,
                buffer.content,
                buffer.content.len() as i64,
                buffer.buffer_fingerprint,
                buffer.pending_temp_path,
                now_millis(),
            ],
        )?;
        if inserted != 1 {
            return Err(AppError::InvalidRequest(
                "it has unsaved changes in the editor".into(),
            ));
        }
        // The record must never leave the project unrecoverable.
        if let Err(error) = preflight_dirty_recovery(&transaction, project_id) {
            return Err(match error {
                AppError::ResourceLimit(_) => AppError::ResourceLimit(
                    "too many Files have unsaved changes to replace this one safely".into(),
                ),
                other => other,
            });
        }
        transaction.commit()?;
        Ok(())
    }
}
