use std::path::Path;

use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::{
    error::AppResult,
    harnesses::SIMULATOR_ID,
    project_registry::{canonical_comparison_key, persisted_path_text},
};

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
    transaction.execute(
        "INSERT INTO agent_sessions
         (id, project_id, chat_tab_id, harness_id, model_id, status,
          last_committed_sequence, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'sim-fast', 'idle', 1, ?5, ?5)",
        params![session_id, project_id, tab_id, SIMULATOR_ID, now],
    )?;
    let payload = serde_json::json!({
        "role": "assistant",
        "text": "This project is ready. Choose a harness and begin a persistent chat."
    });
    transaction.execute(
        "INSERT INTO session_events
         (id, session_id, sequence, kind, payload_json, created_at)
         VALUES (?1, ?2, 1, 'message', ?3, ?4)",
        params![
            Uuid::new_v4().to_string(),
            session_id,
            payload.to_string(),
            now
        ],
    )?;
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
