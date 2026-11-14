use sage_core::error::{AppError, AppResult};

use crate::persistence::Database;
use rusqlite::params;

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

    pub(crate) fn remove_side_chat(&self, project: &str, chat: &str) -> AppResult<()> {
        let exists: bool = self.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM chats WHERE project_id=?1 AND id=?2)",
            params![project, chat],
            |row| row.get(0),
        )?;
        if !exists {
            return Ok(());
        }
        self.purge_chat(project, chat, |connection| {
            if is_temporary(connection, chat)? {
                Ok(())
            } else {
                Err(AppError::InvalidRequest("not a temporary Chat".into()))
            }
        })
    }
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
