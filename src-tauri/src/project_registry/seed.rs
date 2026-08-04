use std::path::Path;

use crate::{
    error::AppResult,
    project_registry::{canonical_comparison_key, persisted_path_text},
    session_events::seed_simulator_chat,
};
use rusqlite::{OptionalExtension, params};

pub(super) fn insert_root(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    root_id: &str,
    canonical: &Path,
    ordinal: i64,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO project_roots
         (id, project_id, absolute_path, canonical_key, ordinal)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            root_id,
            project_id,
            persisted_path_text(canonical)?,
            canonical_comparison_key(canonical),
            ordinal
        ],
    )?;
    Ok(())
}

pub(super) fn seed_project(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    tab_id: &str,
    session_id: &str,
    root_id: &str,
    now: i64,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO workbench_tabs
         (id, project_id, kind, title, position, pinned, dirty, status,
          metadata_json, created_at, updated_at)
         VALUES (?1, ?2, 'chat', 'Chat: New project', 0, 1, 0, 'idle',
                 '{}', ?3, ?3)",
        params![tab_id, project_id, now],
    )?;
    let payload = serde_json::json!({
        "role": "assistant",
        "text": "This project is ready. Choose a harness and begin a persistent chat."
    })
    .to_string();
    seed_simulator_chat(transaction, project_id, tab_id, session_id, now, &payload)?;
    transaction.execute(
        "INSERT INTO project_layouts
         (project_id, active_tab_id, selected_root_id,
          selected_relative_path, updated_at)
         VALUES (?1, ?2, ?3, NULL, ?4)",
        params![project_id, tab_id, root_id, now],
    )?;
    transaction.execute(
        "INSERT INTO app_state(singleton, active_project_id, updated_at)
         VALUES (1, ?1, ?2)
         ON CONFLICT(singleton) DO UPDATE SET
           active_project_id = excluded.active_project_id,
           updated_at = excluded.updated_at",
        params![project_id, now],
    )?;
    Ok(())
}

pub(super) fn active_project_in(
    transaction: &rusqlite::Transaction<'_>,
) -> AppResult<Option<String>> {
    Ok(transaction
        .query_row(
            "SELECT active_project_id FROM app_state WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .flatten())
}
