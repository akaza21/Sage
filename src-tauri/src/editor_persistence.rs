mod limits;
pub(crate) mod p02_read;
pub(crate) mod replace_record;
mod snapshot_revision;
pub(crate) mod store;

#[cfg(test)]
use self::store::EditorBufferStore;

use rusqlite::{OptionalExtension, params};
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};
use sage_db::error::SqliteResultExt;

use crate::workbench_restore::validate_resource_for_project;
use sage_db::database::Database;
use sage_domain::{
    editor::{BufferSnapshotAck, BufferSnapshotRequest},
    resource::ResourceRef,
};

pub use self::limits::{MAX_DIRTY_BUFFERS, MAX_TOTAL_DIRTY_BYTES, MAX_TOTAL_DIRTY_METADATA_BYTES};
pub use self::replace_record::ReplaceRecord;
use self::{limits::preflight_dirty_recovery, snapshot_revision::settled_by_stored};

#[derive(Debug, Clone)]
pub struct PersistedEditorBuffer {
    pub resource: ResourceRef,
    pub base_fingerprint: String,
    pub base_byte_len: u64,
    pub base_modified_at: Option<u64>,
    pub line_ending: String,
    pub content: String,
    pub byte_len: u64,
    pub buffer_fingerprint: String,
    pub revision: u64,
    pub pending_temp_path: Option<String>,
}

fn write_editor_buffer(
    database: &Database,
    project_id: &str,
    request: &BufferSnapshotRequest,
    content: &str,
    buffer_fingerprint: &str,
    rebase: bool,
) -> AppResult<BufferSnapshotAck> {
    let byte_len = content.len() as i64;
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction().db()?;
    // A rebase skips both guards: it replaces the base, and its revision
    // may be lower than the row's (the WebView's count restarts when a
    // File is reopened). That is safe only because rebase_editor_buffer
    // runs as a Terminal operation, after every snapshot queued before
    // it and before any queued after; a snapshot that then carries the
    // old base is refused by the base guard.
    let (stored, settled) = settled_by_stored(
        &transaction,
        project_id,
        request,
        buffer_fingerprint,
        rebase,
    )?;
    if let Some(ack) = settled {
        return Ok(ack);
    }
    let (other_count, other_bytes): (i64, i64) = transaction
        .query_row(
            "SELECT COUNT(*), COALESCE(SUM(buffer_byte_len), 0)
             FROM editor_buffers
             WHERE project_id = ?1
               AND NOT (root_id = ?2 AND relative_path = ?3)",
            params![
                project_id,
                request.resource.root_id,
                request.resource.relative_path
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .db()?;
    if !stored && other_count >= MAX_DIRTY_BUFFERS {
        return Err(AppError::ResourceLimit(format!(
            "project already has the {MAX_DIRTY_BUFFERS}-buffer limit"
        )));
    }
    if other_bytes + byte_len > MAX_TOTAL_DIRTY_BYTES {
        return Err(AppError::ResourceLimit(format!(
            "dirty buffers exceed the {} MiB project limit",
            MAX_TOTAL_DIRTY_BYTES / 1024 / 1024
        )));
    }
    let updated_at = now_millis();
    transaction
        .execute(
            "INSERT INTO editor_buffers (
               project_id, root_id, relative_path, base_fingerprint,
               base_byte_len, base_modified_at, line_ending, buffer_content,
               buffer_byte_len, buffer_fingerprint, revision,
               pending_temp_path, updated_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12
             )
             ON CONFLICT(project_id, root_id, relative_path) DO UPDATE SET
               base_fingerprint = excluded.base_fingerprint,
               base_byte_len = excluded.base_byte_len,
               base_modified_at = excluded.base_modified_at,
               line_ending = excluded.line_ending,
               buffer_content = excluded.buffer_content,
               buffer_byte_len = excluded.buffer_byte_len,
               buffer_fingerprint = excluded.buffer_fingerprint,
               revision = excluded.revision,
               pending_temp_path = NULL,
               updated_at = excluded.updated_at",
            params![
                project_id,
                request.resource.root_id,
                request.resource.relative_path,
                request.base.fingerprint,
                request.base.byte_len as i64,
                request.base.modified_at.map(|value| value as i64),
                request.line_ending,
                content,
                byte_len,
                buffer_fingerprint,
                request.revision as i64,
                updated_at,
            ],
        )
        .db()?;
    set_matching_tab_dirty(
        &transaction,
        project_id,
        &request.resource,
        true,
        updated_at,
    )
    .db()?;
    transaction.commit().db()?;
    Ok(BufferSnapshotAck {
        revision: request.revision,
        persisted_at: updated_at as u64,
        dirty: true,
        applied: true,
    })
}

fn delete_buffer_in(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    resource: &ResourceRef,
    updated_at: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "DELETE FROM editor_buffers
         WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3",
        params![project_id, resource.root_id, resource.relative_path],
    )?;
    set_matching_tab_dirty(transaction, project_id, resource, false, updated_at)
}

fn editor_buffer_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedEditorBuffer> {
    Ok(PersistedEditorBuffer {
        resource: ResourceRef {
            root_id: row.get(0)?,
            relative_path: row.get(1)?,
        },
        base_fingerprint: row.get(2)?,
        base_byte_len: row.get::<_, i64>(3)? as u64,
        base_modified_at: row.get::<_, Option<i64>>(4)?.map(|value| value as u64),
        line_ending: row.get(5)?,
        content: row.get(6)?,
        byte_len: row.get::<_, i64>(7)? as u64,
        buffer_fingerprint: row.get(8)?,
        revision: row.get::<_, i64>(9)? as u64,
        pending_temp_path: row.get(10)?,
    })
}

fn set_matching_tab_dirty(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    resource: &ResourceRef,
    dirty: bool,
    updated_at: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE workbench_tabs
         SET dirty = ?4, updated_at = ?5
         WHERE project_id = ?1
           AND json_extract(metadata_json, '$.resource.rootId') = ?2
           AND json_extract(metadata_json, '$.resource.relativePath') = ?3",
        params![
            project_id,
            resource.root_id,
            resource.relative_path,
            i64::from(dirty),
            updated_at
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod recovery_bound_tests {
    use crate::project_registry::create::ProjectCreateStore;
    use std::{fs, path::Path};

    use rusqlite::params;
    use uuid::Uuid;

    use super::*;

    fn fixture() -> (std::path::PathBuf, Database, String, String) {
        let root = std::env::temp_dir().join(format!("sage-buffer-limit-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = crate::database::open_database(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let root_id = project.roots[0].id.clone();
        (root, database, project.id, root_id)
    }

    fn insert_buffer(
        database: &Database,
        project_id: &str,
        root_id: &str,
        path: &str,
        content: &str,
    ) {
        database
            .connection()
            .execute(
                "INSERT INTO editor_buffers
                 (project_id, root_id, relative_path, base_fingerprint,
                  base_byte_len, base_modified_at, line_ending, buffer_content,
                  buffer_byte_len, buffer_fingerprint, revision,
                  pending_temp_path, updated_at)
                 VALUES (?1, ?2, ?3, 'base', 1, NULL, 'lf', ?4, ?5,
                         'buffer', 1, NULL, 1)",
                params![project_id, root_id, path, content, content.len() as i64],
            )
            .unwrap();
    }

    #[test]
    fn corrupt_over_limit_buffer_count_fails_before_recovery_collection() {
        let (root, database, project_id, root_id) = fixture();
        for index in 0..17 {
            insert_buffer(
                &database,
                &project_id,
                &root_id,
                &format!("buffer-{index}.txt"),
                "x",
            );
        }
        assert!(database.dirty_editor_buffers(&project_id, None).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_actual_buffer_bytes_fail_even_when_declared_length_is_small() {
        let (root, database, project_id, root_id) = fixture();
        insert_buffer(
            &database,
            &project_id,
            &root_id,
            "oversized.txt",
            &"x".repeat(MAX_TOTAL_DIRTY_BYTES as usize + 1),
        );
        database
            .connection()
            .execute(
                "UPDATE editor_buffers SET buffer_byte_len = 1 WHERE project_id = ?1",
                [&project_id],
            )
            .unwrap();
        assert!(database.dirty_editor_buffers(&project_id, None).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_identity_and_temp_bytes_count_toward_recovery_limit() {
        let (root, database, project_id, root_id) = fixture();
        insert_buffer(&database, &project_id, &root_id, "oversized.txt", "x");
        database
            .connection()
            .execute(
                "UPDATE editor_buffers
                 SET pending_temp_path = ?2 WHERE project_id = ?1",
                params![
                    project_id,
                    "x".repeat(MAX_TOTAL_DIRTY_METADATA_BYTES as usize)
                ],
            )
            .unwrap();
        assert!(matches!(
            database.dirty_editor_buffers(&project_id, None),
            Err(AppError::ResourceLimit(_))
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exact_content_limit_with_ordinary_metadata_remains_recoverable() {
        let (root, database, project_id, root_id) = fixture();
        insert_buffer(
            &database,
            &project_id,
            &root_id,
            "exact.txt",
            &"x".repeat(MAX_TOTAL_DIRTY_BYTES as usize),
        );
        assert_eq!(
            database
                .dirty_editor_buffers(&project_id, None)
                .unwrap()
                .len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
}
