use rusqlite::params;
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};

use super::{PersistedEditorBuffer, limits::preflight_dirty_recovery, set_matching_tab_dirty};
use crate::{domain::ResourceRef, persistence::Database};

/// Names one Replace's recovery record: its File, the temporary file it
/// names and the replaced text's fingerprint. Each change below touches the
/// record only while all three still match, so a buffer the editor wrote
/// over it in the meantime is left alone.
pub struct ReplaceRecord<'a> {
    pub resource: &'a ResourceRef,
    pub temp_path: &'a str,
    pub fingerprint: &'a str,
}

const MATCHES_RECORD: &str = "project_id = ?1 AND root_id = ?2 AND relative_path = ?3
     AND pending_temp_path = ?4 AND buffer_fingerprint = ?5";

impl Database {
    /// Recovery deletes the temp file, then the record if the File on disk
    /// matches its fingerprint; otherwise the replaced text stays unsaved.
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

    pub fn delete_replace_record(&self, project_id: &str, record: &ReplaceRecord) -> AppResult<()> {
        self.write_connection()?.execute(
            &format!("DELETE FROM editor_buffers WHERE {MATCHES_RECORD}"),
            params![
                project_id,
                record.resource.root_id,
                record.resource.relative_path,
                record.temp_path,
                record.fingerprint
            ],
        )?;
        Ok(())
    }

    /// Neutralized: holds the File's own text, leaving recovery only the temp file.
    pub fn neutralize_replace_record(
        &self,
        project_id: &str,
        record: &ReplaceRecord,
        original: &str,
        original_fingerprint: &str,
    ) -> AppResult<()> {
        self.write_connection()?.execute(
            &format!(
                "UPDATE editor_buffers
                 SET buffer_content = ?6, buffer_byte_len = ?7,
                     buffer_fingerprint = ?8, updated_at = ?9
                 WHERE {MATCHES_RECORD}"
            ),
            params![
                project_id,
                record.resource.root_id,
                record.resource.relative_path,
                record.temp_path,
                record.fingerprint,
                original,
                original.len() as i64,
                original_fingerprint,
                now_millis()
            ],
        )?;
        Ok(())
    }

    /// Uncertain: the swap can't be proven durable, so keep the text as dirty edits.
    pub fn keep_replace_as_unsaved(
        &self,
        project_id: &str,
        record: &ReplaceRecord,
    ) -> AppResult<()> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let updated_at = now_millis();
        let kept = transaction.execute(
            &format!(
                "UPDATE editor_buffers SET pending_temp_path = NULL, updated_at = ?6
                 WHERE {MATCHES_RECORD}"
            ),
            params![
                project_id,
                record.resource.root_id,
                record.resource.relative_path,
                record.temp_path,
                record.fingerprint,
                updated_at
            ],
        )?;
        if kept == 1 {
            set_matching_tab_dirty(&transaction, project_id, record.resource, true, updated_at)?;
        }
        transaction.commit()?;
        Ok(())
    }
}
