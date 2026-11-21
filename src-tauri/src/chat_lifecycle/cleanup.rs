//! Startup reclamation only. Durable discovery and capacity count every Chat.
use crate::chat_purge::ChatPurgeStore;
use rusqlite::{Connection, params};
use sage_core::error::{AppError, AppResult};
use sage_db::database::Database;
use sage_db::error::SqliteResultExt;

const MAX_CLEANUP_CHATS: usize = 512;

// An attachment distinguishes an unselected legacy placeholder from an archive.
// Metadata is disposable today, but refusing *any* metadata/dirty state also
// protects saved input should it be added there. Queues and image ingress are
// already durable input. A historical welcome has a Turn/item (see
// session_events::legacy_welcome), so the literal zero-history rule keeps it.
const EMPTY_CHAT: &str = "
    FROM chats c
    JOIN chat_tabs a ON a.project_id=c.project_id AND a.chat_id=c.id
    JOIN workbench_tabs w ON w.project_id=a.project_id AND w.id=a.tab_id
    JOIN projects p ON p.id=c.project_id
    WHERE c.title_source='default' AND w.kind='chat'
      AND w.pinned=0 AND w.dirty=0 AND w.metadata_json='{}'
      AND NOT EXISTS(SELECT 1 FROM project_layouts l WHERE l.active_tab_id=w.id)
      AND NOT EXISTS(SELECT 1 FROM workbench_tabs side
        WHERE side.project_id=c.project_id AND json_extract(side.metadata_json,'$.parentChatId')=c.id)
      AND NOT EXISTS(SELECT 1 FROM turns WHERE project_id=c.project_id AND chat_id=c.id)
      AND NOT EXISTS(SELECT 1 FROM items WHERE project_id=c.project_id AND chat_id=c.id)
      AND NOT EXISTS(SELECT 1 FROM agent_sessions WHERE project_id=c.project_id AND chat_id=c.id)
      AND NOT EXISTS(SELECT 1 FROM chat_queued_prompts WHERE project_id=c.project_id AND chat_id=c.id)
      AND NOT EXISTS(SELECT 1 FROM chat_image_artifacts WHERE project_id=c.project_id AND chat_id=c.id)
      AND NOT EXISTS(SELECT 1 FROM chat_lineage WHERE child_project_id=c.project_id AND child_chat_id=c.id)
      AND NOT EXISTS(SELECT 1 FROM chat_lineage WHERE parent_project_id=c.project_id AND parent_chat_id=c.id)
";

pub(crate) fn legacy_empty_chats(database: &Database) -> AppResult<usize> {
    // The durable global ceiling bounds the indexed admission scan; only this
    // small batch is materialized. Deletion needs no new marker or migration.
    let candidates = database
        .connection()
        .prepare(&format!(
            "SELECT c.project_id,c.id,p.kind='privateChat' {EMPTY_CHAT} ORDER BY c.id LIMIT ?1"
        ))
        .db()?
        .query_map([MAX_CLEANUP_CHATS as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
            ))
        })
        .db()?
        .collect::<Result<Vec<_>, _>>()
        .db()?;
    let mut deleted = 0;
    for (project, chat, private) in candidates {
        let folder = if private {
            if !uuid::Uuid::parse_str(&project).is_ok_and(|id| id.to_string() == project) {
                continue;
            }
            let folder = crate::private_chat::storage_root(database)?.join(&project);
            // Never classify filenames as scaffolding: unknown files, linked
            // directories and unreadable folders all preserve their Chat.
            if admit_folder(&folder).is_err() {
                continue;
            }
            Some(folder)
        } else {
            None
        };
        let result = database.purge_chat_with_finalizer(
            &project,
            &chat,
            |connection| {
                admit(connection, &project, &chat)?;
                if let Some(folder) = &folder {
                    admit_folder(folder)?;
                }
                Ok(())
            },
            |transaction| {
                if let Some(folder) = &folder {
                    crate::private_chat::reclaim_empty_in(transaction, &project, folder)?;
                }
                Ok(())
            },
        );
        match result {
            Ok(()) => deleted += 1,
            Err(error) => tracing::warn!(
                ?error,
                "legacy empty Chat was preserved; cleanup will retry"
            ),
        }
    }
    Ok(deleted)
}

fn admit_folder(folder: &std::path::Path) -> AppResult<()> {
    match std::fs::symlink_metadata(folder) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => crate::private_chat::require_empty_folder(folder),
    }
}

fn admit(connection: &Connection, project: &str, chat: &str) -> AppResult<()> {
    let empty: bool = connection
        .query_row(
            &format!("SELECT EXISTS(SELECT 1 {EMPTY_CHAT} AND c.project_id=?1 AND c.id=?2)"),
            params![project, chat],
            |row| row.get(0),
        )
        .db()?;
    if !empty {
        return Err(AppError::InvalidRequest(
            "legacy Chat is no longer empty or unselected".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
