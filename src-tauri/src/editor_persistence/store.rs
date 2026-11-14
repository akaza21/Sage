use super::*;

/// Durable editor buffers, on the shared database.
pub(crate) trait EditorBufferStore {
    fn editor_buffer(
        &self,
        project_id: &str,
        resource: &ResourceRef,
    ) -> AppResult<Option<PersistedEditorBuffer>>;

    fn dirty_editor_buffers(
        &self,
        project_id: &str,
        root_id: Option<&str>,
    ) -> AppResult<Vec<PersistedEditorBuffer>>;

    fn persist_editor_buffer(
        &self,
        project_id: &str,
        request: &BufferSnapshotRequest,
        content: &str,
        buffer_fingerprint: &str,
    ) -> AppResult<BufferSnapshotAck>;

    /// "Keep mine": writes the buffer on a new base, `request.base`, in one
    /// transaction. Unlike a snapshot it replaces the durable base instead of
    /// refusing a different one; the caller has checked that base is on disk.
    fn rebase_editor_buffer(
        &self,
        project_id: &str,
        request: &BufferSnapshotRequest,
        content: &str,
        buffer_fingerprint: &str,
    ) -> AppResult<BufferSnapshotAck>;

    fn apply_clean_editor_snapshot(
        &self,
        project_id: &str,
        request: &BufferSnapshotRequest,
    ) -> AppResult<BufferSnapshotAck>;

    fn set_editor_pending_temp(
        &self,
        project_id: &str,
        resource: &ResourceRef,
        temp_path: &str,
    ) -> AppResult<()>;

    fn clear_editor_pending_temp(&self, project_id: &str, resource: &ResourceRef) -> AppResult<()>;

    fn delete_editor_buffer(&self, project_id: &str, resource: &ResourceRef) -> AppResult<()>;
}

impl EditorBufferStore for Database {
    fn editor_buffer(
        &self,
        project_id: &str,
        resource: &ResourceRef,
    ) -> AppResult<Option<PersistedEditorBuffer>> {
        self.connection()
            .query_row(
                "SELECT root_id, relative_path, base_fingerprint, base_byte_len,
                        base_modified_at, line_ending, buffer_content,
                        buffer_byte_len, buffer_fingerprint, revision,
                        pending_temp_path
                 FROM editor_buffers
                 WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3",
                params![project_id, resource.root_id, resource.relative_path],
                editor_buffer_from_row,
            )
            .optional()
            .db()
    }

    fn dirty_editor_buffers(
        &self,
        project_id: &str,
        root_id: Option<&str>,
    ) -> AppResult<Vec<PersistedEditorBuffer>> {
        let connection = self.connection();
        preflight_dirty_recovery(&connection, project_id)?;
        let sql = if root_id.is_some() {
            "SELECT root_id, relative_path, base_fingerprint, base_byte_len,
                    base_modified_at, line_ending, buffer_content,
                    buffer_byte_len, buffer_fingerprint, revision,
                    pending_temp_path
             FROM editor_buffers
             WHERE project_id = ?1 AND root_id = ?2
             ORDER BY updated_at DESC
             LIMIT 17"
        } else {
            "SELECT root_id, relative_path, base_fingerprint, base_byte_len,
                    base_modified_at, line_ending, buffer_content,
                    buffer_byte_len, buffer_fingerprint, revision,
                    pending_temp_path
             FROM editor_buffers
             WHERE project_id = ?1
             ORDER BY updated_at DESC
             LIMIT 17"
        };
        let mut statement = connection.prepare(sql).db()?;
        let mapped = if let Some(root_id) = root_id {
            statement
                .query_map(params![project_id, root_id], editor_buffer_from_row)
                .db()?
        } else {
            statement
                .query_map(params![project_id], editor_buffer_from_row)
                .db()?
        };
        let buffers = mapped.collect::<Result<Vec<_>, _>>().db()?;
        if buffers.len() > MAX_DIRTY_BUFFERS as usize {
            return Err(AppError::ResourceLimit(format!(
                "project exceeds its {MAX_DIRTY_BUFFERS}-buffer recovery limit"
            )));
        }
        if buffers
            .iter()
            .any(|buffer| buffer.content.len() as u64 != buffer.byte_len)
        {
            return Err(AppError::InvalidRequest(
                "dirty-buffer byte metadata is corrupt".into(),
            ));
        }
        drop(statement);
        drop(connection);
        for buffer in &buffers {
            let connection = self.connection();
            validate_resource_for_project(&connection, project_id, &buffer.resource)?;
        }
        Ok(buffers)
    }

    fn persist_editor_buffer(
        &self,
        project_id: &str,
        request: &BufferSnapshotRequest,
        content: &str,
        buffer_fingerprint: &str,
    ) -> AppResult<BufferSnapshotAck> {
        write_editor_buffer(
            self,
            project_id,
            request,
            content,
            buffer_fingerprint,
            false,
        )
    }

    fn rebase_editor_buffer(
        &self,
        project_id: &str,
        request: &BufferSnapshotRequest,
        content: &str,
        buffer_fingerprint: &str,
    ) -> AppResult<BufferSnapshotAck> {
        write_editor_buffer(self, project_id, request, content, buffer_fingerprint, true)
    }

    fn apply_clean_editor_snapshot(
        &self,
        project_id: &str,
        request: &BufferSnapshotRequest,
    ) -> AppResult<BufferSnapshotAck> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction().db()?;
        // A clean snapshot's text is the base's.
        let (stored, settled) = settled_by_stored(
            &transaction,
            project_id,
            request,
            &request.base.fingerprint,
            false,
        )?;
        if let Some(ack) = settled {
            return Ok(ack);
        }
        if stored {
            delete_buffer_in(&transaction, project_id, &request.resource, now_millis()).db()?;
        }
        let persisted_at = now_millis();
        transaction.commit().db()?;
        Ok(BufferSnapshotAck {
            revision: request.revision,
            persisted_at: persisted_at as u64,
            dirty: false,
            applied: true,
        })
    }

    fn set_editor_pending_temp(
        &self,
        project_id: &str,
        resource: &ResourceRef,
        temp_path: &str,
    ) -> AppResult<()> {
        let updated = self
            .write_connection()?
            .execute(
                "UPDATE editor_buffers
             SET pending_temp_path = ?4, updated_at = ?5
             WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3",
                params![
                    project_id,
                    resource.root_id,
                    resource.relative_path,
                    temp_path,
                    now_millis()
                ],
            )
            .db()?;
        if updated != 1 {
            return Err(AppError::InvalidRequest(
                "dirty buffer disappeared before save".into(),
            ));
        }
        Ok(())
    }

    fn clear_editor_pending_temp(&self, project_id: &str, resource: &ResourceRef) -> AppResult<()> {
        self.write_connection()?
            .execute(
                "UPDATE editor_buffers
             SET pending_temp_path = NULL, updated_at = ?4
             WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3",
                params![
                    project_id,
                    resource.root_id,
                    resource.relative_path,
                    now_millis()
                ],
            )
            .db()?;
        Ok(())
    }

    fn delete_editor_buffer(&self, project_id: &str, resource: &ResourceRef) -> AppResult<()> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction().db()?;
        delete_buffer_in(&transaction, project_id, resource, now_millis()).db()?;
        transaction.commit().db()?;
        Ok(())
    }
}
