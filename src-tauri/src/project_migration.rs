mod limits;
mod roots;

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};
use sage_db::error::SqliteResultExt;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    project_registry::{
        MAX_PROJECTS, MAX_ROOTS_PER_PROJECT, canonical_comparison_key, persisted_path_text,
    },
    workspace::validate_contract_path,
};

use self::limits::{
    MAX_TOTAL_LEGACY_BUFFERS, MAX_TOTAL_LEGACY_ROOTS, MAX_TOTAL_LEGACY_TABS,
    preflight_legacy_bounds,
};
use self::roots::{MigratedResourceSpellings, canonical_migrated_path, canonical_or_recorded_root};

const PROJECT_LIFECYCLE_SCHEMA: &str = include_str!("../migrations/004_project_lifecycle.sql");

struct LegacyRoot {
    project_id: String,
    absolute_path: String,
    ordinal: i64,
}

struct LegacyBuffer {
    project_id: String,
    relative_path: String,
    base_fingerprint: String,
    base_byte_len: i64,
    base_modified_at: Option<i64>,
    line_ending: String,
    buffer_content: String,
    buffer_byte_len: i64,
    buffer_fingerprint: String,
    revision: i64,
    pending_temp_path: Option<String>,
    updated_at: i64,
}

pub fn migrate(connection: &mut Connection) -> AppResult<()> {
    let applied = connection
        .query_row(
            "SELECT EXISTS(
           SELECT 1 FROM schema_migrations WHERE version = 4
         )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .db()?;
    if applied {
        return Ok(());
    }

    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .db()?;
    preflight_legacy_bounds(&transaction)?;
    transaction.execute_batch(PROJECT_LIFECYCLE_SCHEMA).db()?;

    let roots = {
        let mut statement = transaction
            .prepare(
                "SELECT project_id, absolute_path, ordinal
             FROM project_roots
             ORDER BY project_id, ordinal
             LIMIT ?1",
            )
            .db()?;
        statement
            .query_map([MAX_TOTAL_LEGACY_ROOTS + 1], |row| {
                Ok(LegacyRoot {
                    project_id: row.get(0)?,
                    absolute_path: row.get(1)?,
                    ordinal: row.get(2)?,
                })
            })
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?
    };
    let project_ids = {
        let mut statement = transaction
            .prepare("SELECT id FROM projects LIMIT ?1")
            .db()?;
        statement
            .query_map([MAX_PROJECTS + 1], |row| row.get::<_, String>(0))
            .db()?
            .collect::<Result<HashSet<_>, _>>()
            .db()?
    };
    if project_ids.len() > MAX_PROJECTS as usize {
        return Err(AppError::ResourceLimit(format!(
            "legacy registry exceeds its {MAX_PROJECTS}-project limit"
        )));
    }
    let mut root_counts = HashMap::new();
    for root in &roots {
        *root_counts
            .entry(root.project_id.as_str())
            .or_insert(0_usize) += 1;
    }
    if project_ids
        .iter()
        .any(|project_id| root_counts.get(project_id.as_str()).copied().unwrap_or(0) == 0)
    {
        return Err(AppError::InvalidRequest(
            "legacy project has no folder root".into(),
        ));
    }
    if root_counts
        .values()
        .any(|count| *count > MAX_ROOTS_PER_PROJECT as usize)
    {
        return Err(AppError::ResourceLimit(format!(
            "legacy project exceeds its {MAX_ROOTS_PER_PROJECT}-root limit"
        )));
    }

    let mut first_roots = HashMap::new();
    let mut seen_keys = HashSet::new();
    for root in roots {
        let canonical = canonical_or_recorded_root(&root.absolute_path)?;
        let root_id = Uuid::new_v4().to_string();
        let canonical_key = canonical_comparison_key(&canonical);
        if !seen_keys.insert((root.project_id.clone(), canonical_key.clone())) {
            return Err(AppError::InvalidRequest(format!(
                "legacy project {} contains duplicate canonical roots",
                root.project_id
            )));
        }
        first_roots
            .entry(root.project_id.clone())
            .or_insert_with(|| (root_id.clone(), canonical.clone()));
        transaction
            .execute(
                "INSERT INTO project_roots_v4
             (id, project_id, absolute_path, canonical_key, ordinal)
             VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    root_id,
                    root.project_id,
                    persisted_path_text(&canonical)?,
                    canonical_key,
                    root.ordinal
                ],
            )
            .db()?;
    }

    let mut resource_spellings = MigratedResourceSpellings::default();
    let buffers = load_legacy_buffers(&transaction)?;
    for buffer in buffers {
        let (root_id, root_path) = first_roots.get(&buffer.project_id).ok_or_else(|| {
            AppError::InvalidRequest(format!(
                "legacy dirty buffer {} has no project root",
                buffer.relative_path
            ))
        })?;
        let relative_path = canonical_migrated_path(
            &buffer.project_id,
            root_id,
            root_path,
            &buffer.relative_path,
        )?;
        resource_spellings.record(root_id, &relative_path)?;
        transaction
            .execute(
                "INSERT INTO editor_buffers_v4 (
               project_id, root_id, relative_path, base_fingerprint,
               base_byte_len, base_modified_at, line_ending, buffer_content,
               buffer_byte_len, buffer_fingerprint, revision,
               pending_temp_path, updated_at
             ) VALUES (
               ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
             )",
                params![
                    buffer.project_id,
                    root_id,
                    relative_path,
                    buffer.base_fingerprint,
                    buffer.base_byte_len,
                    buffer.base_modified_at,
                    buffer.line_ending,
                    buffer.buffer_content,
                    buffer.buffer_byte_len,
                    buffer.buffer_fingerprint,
                    buffer.revision,
                    buffer.pending_temp_path,
                    buffer.updated_at
                ],
            )
            .db()?;
    }

    migrate_tab_resources(&transaction, &first_roots, &mut resource_spellings)?;
    transaction
        .execute_batch(
            "DROP TABLE editor_buffers;
         DROP TABLE project_roots;
         ALTER TABLE project_roots_v4 RENAME TO project_roots;
         ALTER TABLE editor_buffers_v4 RENAME TO editor_buffers;
         CREATE INDEX idx_editor_buffers_project_updated
           ON editor_buffers(project_id, updated_at);
         CREATE INDEX idx_project_roots_project_ordinal
           ON project_roots(project_id, ordinal);",
        )
        .db()?;

    seed_layouts(&transaction).db()?;
    transaction
        .execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (4, ?1)",
            [now_millis()],
        )
        .db()?;

    let foreign_key_error = transaction
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
        .optional()
        .db()?
        .unwrap_or(false);
    if foreign_key_error {
        return Err(AppError::InvalidRequest(
            "project migration failed its foreign-key check".into(),
        ));
    }
    transaction.commit().db()?;
    Ok(())
}

fn load_legacy_buffers(transaction: &rusqlite::Transaction<'_>) -> AppResult<Vec<LegacyBuffer>> {
    let mut statement = transaction
        .prepare(
            "SELECT project_id, relative_path, base_fingerprint, base_byte_len,
                base_modified_at, line_ending, buffer_content,
                buffer_byte_len, buffer_fingerprint, revision,
                pending_temp_path, updated_at
         FROM editor_buffers
         LIMIT ?1",
        )
        .db()?;
    let buffers = statement
        .query_map([MAX_TOTAL_LEGACY_BUFFERS + 1], |row| {
            Ok(LegacyBuffer {
                project_id: row.get(0)?,
                relative_path: row.get(1)?,
                base_fingerprint: row.get(2)?,
                base_byte_len: row.get(3)?,
                base_modified_at: row.get(4)?,
                line_ending: row.get(5)?,
                buffer_content: row.get(6)?,
                buffer_byte_len: row.get(7)?,
                buffer_fingerprint: row.get(8)?,
                revision: row.get(9)?,
                pending_temp_path: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .db()?
        .collect::<Result<Vec<_>, _>>()
        .db()?;
    if buffers.len() > MAX_TOTAL_LEGACY_BUFFERS as usize {
        return Err(AppError::ResourceLimit(
            "legacy dirty-buffer count exceeds the migration limit".into(),
        ));
    }
    for buffer in &buffers {
        validate_contract_path(&buffer.relative_path, false)?;
        if buffer.buffer_content.len() as i64 != buffer.buffer_byte_len {
            return Err(AppError::InvalidRequest(
                "legacy dirty-buffer byte metadata is corrupt".into(),
            ));
        }
    }
    Ok(buffers)
}

fn migrate_tab_resources(
    transaction: &rusqlite::Transaction<'_>,
    first_roots: &HashMap<String, (String, PathBuf)>,
    resource_spellings: &mut MigratedResourceSpellings,
) -> AppResult<()> {
    let tabs = {
        let mut statement = transaction
            .prepare(
                "SELECT id, project_id, kind, metadata_json
                 FROM workbench_tabs LIMIT ?1",
            )
            .db()?;
        statement
            .query_map([MAX_TOTAL_LEGACY_TABS + 1], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?
    };
    if tabs.len() > MAX_TOTAL_LEGACY_TABS as usize {
        return Err(AppError::ResourceLimit(
            "legacy tab count exceeds the migration limit".into(),
        ));
    }

    for (tab_id, project_id, kind, metadata) in tabs {
        let mut metadata: Value = serde_json::from_str(&metadata)
            .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
        let Some(relative_path) = metadata
            .get("resourcePath")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            if kind == "file" || kind == "diff" {
                return Err(AppError::InvalidRequest(format!(
                    "legacy {kind} tab {tab_id} has no resource path"
                )));
            }
            continue;
        };
        if kind != "file" && kind != "diff" {
            return Err(AppError::InvalidRequest(format!(
                "legacy non-resource tab {tab_id} carries a resource path"
            )));
        }
        let (root_id, root_path) = first_roots.get(&project_id).ok_or_else(|| {
            AppError::InvalidRequest(format!("legacy resource tab {tab_id} has no project root"))
        })?;
        let relative_path =
            canonical_migrated_path(&project_id, root_id, root_path, &relative_path)?;
        resource_spellings.record(root_id, &relative_path)?;
        let object = metadata.as_object_mut().ok_or_else(|| {
            AppError::InvalidRequest(format!("tab {tab_id} metadata is not an object"))
        })?;
        object.remove("resourcePath");
        object.insert(
            "resource".into(),
            json!({ "rootId": root_id, "relativePath": relative_path }),
        );
        transaction
            .execute(
                "UPDATE workbench_tabs SET metadata_json = ?2 WHERE id = ?1",
                params![
                    tab_id,
                    serde_json::to_string(&metadata)
                        .map_err(|error| AppError::InvalidRequest(error.to_string()))?
                ],
            )
            .db()?;
    }
    Ok(())
}

fn seed_layouts(transaction: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let projects = {
        let mut statement =
            transaction.prepare("SELECT id FROM projects ORDER BY last_opened_at DESC LIMIT ?1")?;
        statement
            .query_map([MAX_PROJECTS + 1], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    for project_id in &projects {
        let active_tab_id = transaction
            .query_row(
                "SELECT id FROM workbench_tabs
                 WHERE project_id = ?1
                 ORDER BY CASE WHEN kind = 'chat' THEN 0 ELSE 1 END, position
                 LIMIT 1",
                [project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let selected_root_id = transaction
            .query_row(
                "SELECT id FROM project_roots
                 WHERE project_id = ?1 ORDER BY ordinal LIMIT 1",
                [project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        transaction.execute(
            "INSERT INTO project_layouts
             (project_id, active_tab_id, selected_root_id,
              selected_relative_path, updated_at)
             VALUES (?1, ?2, ?3, NULL, ?4)",
            params![project_id, active_tab_id, selected_root_id, now_millis()],
        )?;
    }
    transaction.execute(
        "INSERT INTO app_state(singleton, active_project_id, updated_at)
         VALUES (1, ?1, ?2)",
        params![projects.first(), now_millis()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
