use crate::{
    error::{AppError, AppResult},
    persistence::Database,
};
use rusqlite::{OptionalExtension, params};

pub(crate) fn validate_parent(
    transaction: &rusqlite::Transaction<'_>,
    project: &str,
    parent: &str,
) -> AppResult<()> {
    crate::chat_lifecycle::validate_chat_identifier("Parent Chat ID", parent)?;
    let count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM chats WHERE id IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1)",
        [],
        |row| row.get(0),
    )?;
    let exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM chats WHERE project_id=?1 AND id=?2 AND id NOT IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1))",
        params![project, parent],
        |row| row.get(0),
    )?;
    let project_has_side: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM chats WHERE project_id=?1 AND id IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1))",
        [project],
        |row| row.get(0),
    )?;
    if !exists || project_has_side || count >= 8 {
        return Err(AppError::InvalidRequest("Close the existing side chat before opening another; up to eight Projects may own a temporary Chat".into()));
    }
    Ok(())
}

pub(crate) fn is_temporary(connection: &rusqlite::Connection, chat_id: &str) -> AppResult<bool> {
    Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM workbench_tabs WHERE id=?1 AND kind='chat' AND json_extract(metadata_json,'$.ephemeral')=1)", [chat_id], |row| row.get(0))?)
}

pub(crate) fn refuse_persistence(
    connection: &rusqlite::Connection,
    chat_id: &str,
) -> AppResult<()> {
    if is_temporary(connection, chat_id)? {
        return Err(AppError::InvalidRequest(
            "Side chats are temporary and cannot be persisted".into(),
        ));
    }
    Ok(())
}

pub(crate) fn refuse_parent_removal(
    connection: &rusqlite::Connection,
    project: &str,
    chat: &str,
) -> AppResult<()> {
    let owns_side: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM workbench_tabs WHERE project_id=?1 AND id IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1) AND json_extract(metadata_json,'$.parentChatId')=?2)",
        params![project, chat], |row| row.get(0),
    )?;
    if owns_side {
        return Err(AppError::InvalidRequest(
            "Close this Chat's side chat before moving or archiving it".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_layout(
    connection: &rusqlite::Connection,
    layout: &crate::domain::ProjectLayoutRequest,
) -> AppResult<()> {
    let rows = connection.prepare("SELECT id,json_extract(metadata_json,'$.parentChatId') FROM workbench_tabs WHERE project_id=?1 AND id IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1) LIMIT 9")?
        .query_map([&layout.project_id], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))?
        .collect::<Result<Vec<_>,_>>()?;
    if rows.len() > 8
        || rows.iter().any(|(side, parent)| {
            [side, parent].iter().any(|id| {
                !layout
                    .tabs
                    .iter()
                    .any(|tab| tab.kind == "chat" && &tab.id == *id)
            })
        })
    {
        return Err(AppError::InvalidRequest(
            "Close the side chat before removing it or its parent from the layout".into(),
        ));
    }
    Ok(())
}

/// The immutable-history guards a temporary Chat purge lifts and restores.
const HISTORY_GUARDS: [&str; 8] = [
    "deny_activity_delete",
    "deny_agent_session_delete",
    "deny_chat_delete",
    "deny_fact_delete",
    "deny_item_delete",
    "deny_runtime_epoch_delete",
    "deny_server_request_delete",
    "deny_turn_delete",
];
/// Rows keyed by (project_id, chat_id), deleted while the guards are lifted.
const CHAT_ROWS: [&str; 14] = [
    "acp_child_thread_transcripts",
    "acp_child_thread_bindings",
    "acp_child_thread_owners",
    "acp_turn_usage_snapshots",
    "acp_turn_terminal_reasons",
    "chat_image_artifacts",
    "chat_queued_prompts",
    "server_requests",
    "session_facts",
    "session_activities",
    "items",
    "turns",
    "session_runtime_epochs",
    "chat_session_heads",
];
/// Image artifacts leave in bounded batches, each its own short transaction.
const IMAGE_BATCH: usize = 500;

impl Database {
    /// Removes every temporary Chat. One failed purge does not stop the
    /// others; the first error is returned after all were attempted.
    pub(crate) fn purge_side_chats(&self) -> AppResult<()> {
        let rows = self
            .connection()
            .prepare("SELECT project_id,id FROM chats WHERE id IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1) ORDER BY id LIMIT 9")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut failure = (rows.len() > 8)
            .then(|| AppError::ResourceLimit("temporary Chat registry exceeds eight".into()));
        for (project, chat) in rows.into_iter().take(8) {
            if let Err(error) = self.remove_side_chat(&project, &chat) {
                tracing::warn!(?error, "temporary Chat purge failed; it will retry later");
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Deletes a temporary Chat's image artifacts `batch` rows at a time and
    /// then their files. Nothing references these rows, and a crash between
    /// a batch and its files leaves only orphans that startup reconciles.
    pub(crate) fn remove_side_chat_images(
        &self,
        project: &str,
        chat: &str,
        batch: usize,
    ) -> AppResult<usize> {
        let mut removed = 0;
        loop {
            let ids = {
                let mut connection = self.write_connection()?;
                let transaction = connection.transaction()?;
                let ids = transaction
                    .prepare("SELECT id FROM chat_image_artifacts WHERE project_id=?1 AND chat_id=?2 ORDER BY id LIMIT ?3")?
                    .query_map(params![project, chat, batch as i64], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                for id in &ids {
                    transaction.execute("DELETE FROM chat_image_artifacts WHERE id=?1", [id])?;
                }
                transaction.commit()?;
                ids
            };
            if ids.is_empty() {
                return Ok(removed);
            }
            removed += ids.len();
            if let Ok(root) = self.image_artifact_root() {
                for id in ids {
                    if uuid::Uuid::parse_str(&id).is_ok_and(|uuid| uuid.to_string() == id) {
                        for extension in ["full", "thumb"] {
                            let _ = crate::acp::migration::remove_image_file(
                                &root.join(format!("{id}.{extension}")),
                            );
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn remove_side_chat(&self, project: &str, chat: &str) -> AppResult<()> {
        let exists: bool = self.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM chats WHERE project_id=?1 AND id=?2)",
            params![project, chat],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(());
        }
        if !is_temporary(&self.connection(), chat)? {
            return Err(AppError::InvalidRequest("not a temporary Chat".into()));
        }
        // Refuse before touching anything when a guard is already missing.
        history_guards(&self.connection())?;
        self.remove_side_chat_images(project, chat, IMAGE_BATCH)?;
        // An image row inserted after the batches is deleted with the Chat
        // below; its files become orphans that startup reconciliation
        // (image_artifacts::reconcile_image_artifacts) removes.
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        transaction.pragma_update(None, "defer_foreign_keys", true)?;
        let guards = history_guards(&transaction)?;
        for (name, _) in &guards {
            transaction.execute_batch(&format!("DROP TRIGGER \"{name}\""))?;
        }
        for table in CHAT_ROWS {
            transaction.execute(
                &format!("DELETE FROM {table} WHERE project_id=?1 AND chat_id=?2"),
                params![project, chat],
            )?;
        }
        let (sessions, turns, facts, bytes): (i64,i64,i64,i64) = transaction.query_row("SELECT COUNT(*),COALESCE(SUM(turn_count),0),COALESCE(SUM(fact_count),0),COALESCE(SUM(fact_bytes),0) FROM acp_session_usage WHERE project_id=?1 AND chat_id=?2", params![project,chat], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)))?;
        transaction.execute(
            "DELETE FROM acp_session_usage WHERE project_id=?1 AND chat_id=?2",
            params![project, chat],
        )?;
        transaction.execute(
            "DELETE FROM agent_sessions WHERE project_id=?1 AND chat_id=?2",
            params![project, chat],
        )?;
        transaction.execute(
            "DELETE FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
            params![project, chat],
        )?;
        transaction.execute(
            "DELETE FROM workbench_tabs WHERE project_id=?1 AND id=?2",
            params![project, chat],
        )?;
        transaction.execute(
            "DELETE FROM chats WHERE project_id=?1 AND id=?2",
            params![project, chat],
        )?;
        transaction.execute("UPDATE acp_project_usage SET chat_count=chat_count-1,session_count=session_count-?2,turn_count=turn_count-?3,fact_count=fact_count-?4,payload_bytes=payload_bytes-?5,revision=revision+1 WHERE project_id=?1", params![project,sessions,turns,facts,bytes])?;
        transaction.execute("UPDATE acp_store_usage SET chat_count=chat_count-1,session_count=session_count-?1,turn_count=turn_count-?2,fact_count=fact_count-?3,fact_bytes=fact_bytes-?4,shared_payload_bytes=shared_payload_bytes-?4,revision=revision+1 WHERE singleton=1", params![sessions,turns,facts,bytes])?;
        let fallback: Option<String> = transaction.query_row("SELECT id FROM workbench_tabs WHERE project_id=?1 AND kind='chat' AND id NOT IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1) ORDER BY position LIMIT 1", [project], |row| row.get(0)).optional()?;
        transaction.execute(
            "UPDATE project_layouts SET active_tab_id=?3 WHERE project_id=?1 AND active_tab_id=?2",
            params![project, chat, fallback],
        )?;
        for (_, sql) in guards {
            transaction.execute_batch(&sql)?;
        }
        // foreign_keys is ON and checks are deferred, so COMMIT itself refuses
        // a purge that would leave a dangling reference, without a table scan:
        // AppError::Database(SqliteFailure, extended code 787 FOREIGNKEY). The
        // dropped transaction then rolls back, restoring the lifted guards.
        transaction.commit()?;
        Ok(())
    }
}

/// Each guard by name, with the SQL that restores it; a missing one names itself.
fn history_guards(connection: &rusqlite::Connection) -> AppResult<Vec<(&'static str, String)>> {
    HISTORY_GUARDS
        .into_iter()
        .map(|name| {
            connection
                .query_row(
                    "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
                    [name],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|sql| (name, sql))
                .ok_or_else(|| {
                    AppError::InvalidRequest(format!(
                        "immutable history guard {name} is missing; temporary Chat cleanup refused"
                    ))
                })
        })
        .collect()
}

#[cfg(test)]
#[path = "side_chat_storage_tests.rs"]
mod purge_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temporary_chats_are_hidden_deleted_and_durable_guards_are_restored() {
        let root = std::env::temp_dir().join(format!("sage-side-chat-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = Database::open(&root.join("sage.db")).unwrap();
        database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project = snapshot.project.as_ref().unwrap().id.clone();
        let parent = snapshot.active_tab_id.clone().unwrap();
        let layout = crate::domain::ProjectLayoutRequest {
            project_id: project.clone(),
            tabs: snapshot.tabs,
            active_tab_id: Some(parent.clone()),
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        };
        let side = database
            .create_chat_with_parent(&project, &layout, Some(&parent))
            .unwrap();
        assert!(side.starts_with("side-"));
        assert!(refuse_parent_removal(&database.connection(), &project, &parent).is_err());
        assert!(validate_layout(&database.connection(), &layout).is_err());
        assert!(
            database
                .project_list()
                .unwrap()
                .iter()
                .all(|project| project.rail_chats.iter().all(|chat| chat.id != side))
        );
        assert!(database.remove_side_chat(&project, &parent).is_err());
        let artifact_root = database.image_artifact_root().unwrap();
        let parent_image = crate::image_artifacts::insert_test_image(
            &database.connection(),
            artifact_root,
            &project,
            &parent,
            "parent.png",
        );
        let side_image = crate::image_artifacts::insert_test_image(
            &database.connection(),
            artifact_root,
            &project,
            &side,
            "side.png",
        );
        database.remove_side_chat(&project, &side).unwrap();
        assert!(refuse_parent_removal(&database.connection(), &project, &parent).is_ok());
        assert!(validate_layout(&database.connection(), &layout).is_ok());
        assert!(
            artifact_root
                .join(format!("{}.full", parent_image.artifact_id))
                .is_file()
        );
        assert!(
            !artifact_root
                .join(format!("{}.full", side_image.artifact_id))
                .exists()
        );
        assert!(database.connection().query_row("SELECT EXISTS(SELECT 1 FROM chat_image_artifacts WHERE id=?1 AND lifecycle='pending')", [&parent_image.artifact_id], |row| row.get::<_,bool>(0)).unwrap());
        assert!(
            !database
                .connection()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM chats WHERE id=?1)",
                    [&side],
                    |row| row.get::<_, bool>(0)
                )
                .unwrap()
        );
        assert!(
            database
                .connection()
                .execute("DELETE FROM chats WHERE id=?1", [&parent])
                .is_err()
        );
        let layout = crate::domain::ProjectLayoutRequest {
            active_tab_id: Some(parent.clone()),
            ..layout
        };
        database
            .create_chat_with_parent(&project, &layout, Some(&parent))
            .unwrap();
        drop(database);
        let reopened = Database::open(&root.join("sage.db")).unwrap();
        assert_eq!(
            reopened
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM chats WHERE id IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1)",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
