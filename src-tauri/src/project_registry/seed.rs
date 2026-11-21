use std::path::Path;

use sage_core::error::AppResult;
use sage_db::error::SqliteResultExt;

use crate::project_registry::{conservative_overlap_key, persisted_path_text};
use rusqlite::{OptionalExtension, params};

pub(crate) fn insert_root(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    root_id: &str,
    canonical: &Path,
    ordinal: i64,
) -> AppResult<()> {
    transaction
        .execute(
            "INSERT INTO project_roots
         (id, project_id, absolute_path, canonical_key, ordinal)
         VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                root_id,
                project_id,
                persisted_path_text(canonical)?,
                conservative_overlap_key(canonical),
                ordinal
            ],
        )
        .db()?;
    Ok(())
}

pub(crate) fn seed_project(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    tab_id: &str,
    root_id: Option<&str>,
    now: i64,
) -> AppResult<()> {
    seed_empty_project(transaction, project_id, root_id, now)?;
    crate::chat_lifecycle::create::insert_chat_with_identity(
        transaction,
        project_id,
        false,
        None,
        Some(tab_id),
    )?;
    Ok(())
}

pub(crate) fn seed_empty_project(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    root_id: Option<&str>,
    now: i64,
) -> AppResult<()> {
    transaction.execute("INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision) VALUES (?1,0,0,0,0,0,0,0)", [project_id]).db()?;
    transaction.execute("INSERT INTO project_layouts(project_id,active_tab_id,selected_root_id,selected_relative_path,updated_at) VALUES (?1,NULL,?2,NULL,?3)", params![project_id,root_id,now]).db()?;
    transaction.execute("INSERT INTO app_state(singleton,active_project_id,updated_at) VALUES (1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET active_project_id=excluded.active_project_id,updated_at=excluded.updated_at", params![project_id,now]).db()?;
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
        .optional()
        .db()?
        .flatten())
}

#[cfg(test)]
mod tests;
