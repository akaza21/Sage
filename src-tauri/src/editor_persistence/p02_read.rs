use rusqlite::{OptionalExtension, params};

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{
    domain::ResourceRef,
    error::{AppError, AppResult},
    persistence::Database,
};

#[cfg(test)]
static REVISE_AFTER_PREFLIGHT_FOR_TEST: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub(crate) fn revise_after_preflight_for_test(enabled: bool) {
    REVISE_AFTER_PREFLIGHT_FOR_TEST.store(enabled, Ordering::SeqCst);
}

impl Database {
    /// Reads only durable dirty-buffer identity/revision. This is the P-02
    /// barrier authority and deliberately never materializes buffer content.
    pub(crate) fn editor_buffer_revision(
        &self,
        project_id: &str,
        resource: &ResourceRef,
    ) -> AppResult<Option<u64>> {
        let revision = self
            .connection()
            .query_row(
                "SELECT revision FROM editor_buffers
                 WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3",
                params![project_id, resource.root_id, resource.relative_path],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;
        match revision {
            Some(value) if value > 0 => Ok(Some(value as u64)),
            Some(_) => Err(AppError::InvalidRequest(
                "dirty-buffer revision is corrupt".into(),
            )),
            None => Ok(None),
        }
    }

    /// Preflights signed metadata and bounds the SQLite fetch in one read
    /// transaction before Rust receives a buffer-content allocation.
    pub(crate) fn read_editor_buffer_bounded(
        &self,
        project_id: &str,
        resource: &ResourceRef,
        expected_revision: u64,
        max_bytes: usize,
    ) -> AppResult<String> {
        let expected_revision = i64::try_from(expected_revision)
            .map_err(|_| AppError::InvalidRequest("dirty-buffer revision is corrupt".into()))?;
        let max_bytes = i64::try_from(max_bytes)
            .map_err(|_| AppError::ResourceLimit("dirty-buffer cap is invalid".into()))?;
        let mut connection = self.connection();
        let transaction = connection.transaction()?;
        let metadata = transaction
            .query_row(
                "SELECT revision, buffer_byte_len, length(CAST(buffer_content AS BLOB))
                 FROM editor_buffers
                 WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3",
                params![project_id, resource.root_id, resource.relative_path],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidRequest("dirty-buffer disappeared".into()))?;
        if metadata.0 != expected_revision
            || metadata.0 <= 0
            || metadata.1 < 0
            || metadata.2 < 0
            || metadata.1 != metadata.2
            || metadata.1 > max_bytes
        {
            return Err(AppError::InvalidRequest(
                "dirty-buffer metadata is corrupt".into(),
            ));
        }
        #[cfg(test)]
        if REVISE_AFTER_PREFLIGHT_FOR_TEST.swap(false, Ordering::SeqCst) {
            // Narrow test-only interleave: the second guarded fetch must reject
            // a row changed after metadata validation in this same transaction.
            transaction.execute(
                "UPDATE editor_buffers SET revision = revision + 1
                 WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3",
                params![project_id, resource.root_id, resource.relative_path],
            )?;
        }
        let content = transaction
            .query_row(
                "SELECT buffer_content FROM editor_buffers
                 WHERE project_id = ?1 AND root_id = ?2 AND relative_path = ?3
                   AND revision = ?4 AND buffer_byte_len = ?5
                   AND length(CAST(buffer_content AS BLOB)) = ?5
                   AND length(CAST(buffer_content AS BLOB)) <= ?6",
                params![
                    project_id,
                    resource.root_id,
                    resource.relative_path,
                    metadata.0,
                    metadata.1,
                    max_bytes
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidRequest("dirty-buffer changed during read".into()))?;
        if i64::try_from(content.len()).ok() != Some(metadata.1) || content.as_bytes().contains(&0)
        {
            return Err(AppError::InvalidRequest(
                "dirty-buffer content is corrupt".into(),
            ));
        }
        transaction.commit()?;
        Ok(content)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::params;
    use uuid::Uuid;

    use super::*;

    fn fixture() -> (std::path::PathBuf, Database, String, ResourceRef) {
        let root = std::env::temp_dir().join(format!("sage-p02-row-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let resource = ResourceRef {
            root_id: project.roots[0].id.clone(),
            relative_path: "file.txt".into(),
        };
        database.connection().execute(
            "INSERT INTO editor_buffers (project_id, root_id, relative_path, base_fingerprint, base_byte_len, base_modified_at, line_ending, buffer_content, buffer_byte_len, buffer_fingerprint, revision, pending_temp_path, updated_at)
             VALUES (?1, ?2, ?3, 'base', 0, NULL, 'lf', 'valid', 5, 'buffer', 7, NULL, 1)",
            params![project.id, resource.root_id, resource.relative_path],
        ).unwrap();
        (root, database, project.id, resource)
    }

    #[test]
    fn bounded_persisted_matrix_rejects_malformed_rows_and_accepts_exact_cap() {
        let cases: &[(&str, &str)] = &[
            ("revision", "revision = -1"),
            ("byte_len", "buffer_byte_len = -1"),
            ("metadata_mismatch", "buffer_byte_len = 4"),
            ("nul", "buffer_content = char(0), buffer_byte_len = 1"),
        ];
        for (name, update) in cases {
            let (root, database, project, resource) = fixture();
            let write = database.connection().execute(
                &format!("UPDATE editor_buffers SET {update} WHERE project_id = ?1"),
                params![&project],
            );
            // SQLite strict-table checks reject some malformed values before
            // P-02 can observe them; any representable row fails the bounded
            // reader itself.
            if write.is_ok() {
                assert!(
                    database
                        .read_editor_buffer_bounded(&project, &resource, 7, 1024)
                        .is_err(),
                    "{name}"
                );
            }
            fs::remove_dir_all(root).unwrap();
        }

        let (root, database, project, resource) = fixture();
        let cap = 1024 * 1024;
        database.connection().execute(
            "UPDATE editor_buffers SET buffer_content = ?2, buffer_byte_len = ?3 WHERE project_id = ?1",
            params![project, "x".repeat(cap), cap as i64],
        ).unwrap();
        assert_eq!(
            database
                .read_editor_buffer_bounded(&project, &resource, 7, cap)
                .unwrap()
                .len(),
            cap
        );
        database.connection().execute(
            "UPDATE editor_buffers SET buffer_content = ?2, buffer_byte_len = ?3 WHERE project_id = ?1",
            params![project, "x".repeat(cap + 1), (cap + 1) as i64],
        ).unwrap();
        assert!(
            database
                .read_editor_buffer_bounded(&project, &resource, 7, cap)
                .is_err()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bounded_persisted_fetch_rejects_post_preflight_revision_change_and_invalid_blob() {
        let (root, database, project, resource) = fixture();
        revise_after_preflight_for_test(true);
        assert!(
            database
                .read_editor_buffer_bounded(&project, &resource, 7, 1024)
                .is_err()
        );
        // The STRICT TEXT schema rejects arbitrary blobs before this reader;
        // retaining that rejection is the invalid-UTF8/blob fail-closed case.
        assert!(database.connection().execute(
            "UPDATE editor_buffers SET buffer_content = CAST(x'FF' AS BLOB), buffer_byte_len = 1, revision = 7 WHERE project_id = ?1",
            params![&project],
        ).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
