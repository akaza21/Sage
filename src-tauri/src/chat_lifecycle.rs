use rusqlite::params;
use uuid::Uuid;

use crate::{
    acp::limits::LIMITS,
    domain::ProjectLayoutRequest,
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
    workbench_persistence::{MAX_RESTORED_TABS, persist_layout_in},
};

impl Database {
    pub(crate) fn create_chat(&self, current_layout: &ProjectLayoutRequest) -> AppResult<String> {
        if current_layout.tabs.len() >= MAX_RESTORED_TABS {
            return Err(AppError::ResourceLimit(format!(
                "project layout has reached its {MAX_RESTORED_TABS}-tab limit"
            )));
        }
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let active_project_id = transaction.query_row(
            "SELECT active_project_id FROM app_state WHERE singleton=1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )?;
        if active_project_id.as_deref() != Some(current_layout.project_id.as_str()) {
            return Err(AppError::InvalidRequest(
                "Chat layout does not belong to the active Project".into(),
            ));
        }
        persist_layout_in(&transaction, current_layout)?;

        let chat_count: i64 = transaction.query_row(
            "SELECT chat_count FROM acp_project_usage WHERE project_id=?1",
            [&current_layout.project_id],
            |row| row.get(0),
        )?;
        if chat_count >= LIMITS.durable.chats_per_project as i64 {
            return Err(AppError::ResourceLimit(format!(
                "Project has reached its {}-Chat limit",
                LIMITS.durable.chats_per_project
            )));
        }
        let store_chat_count: i64 = transaction.query_row(
            "SELECT chat_count FROM acp_store_usage WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if store_chat_count >= LIMITS.durable.chats_global as i64 {
            return Err(AppError::ResourceLimit(format!(
                "Sage has reached its {}-Chat limit",
                LIMITS.durable.chats_global
            )));
        }

        let ordinal: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(ordinal),-1)+1 FROM chats WHERE project_id=?1",
            [&current_layout.project_id],
            |row| row.get(0),
        )?;
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position),-1)+1 FROM workbench_tabs WHERE project_id=?1",
            [&current_layout.project_id],
            |row| row.get(0),
        )?;
        let chat_id = Uuid::new_v4().to_string();
        let title = format!("Chat: New chat {}", ordinal + 1);
        let now = now_millis();
        transaction.execute(
            "INSERT INTO chats(
               id,project_id,ordinal,title,projection_revision,created_at,updated_at
             ) VALUES (?1,?2,?3,?4,0,?5,?5)",
            params![chat_id, current_layout.project_id, ordinal, title, now],
        )?;
        transaction.execute(
            "INSERT INTO workbench_tabs(
               id,project_id,kind,title,position,pinned,dirty,status,
               metadata_json,created_at,updated_at
             ) VALUES (?1,?2,'chat',?3,?4,0,0,'idle','{}',?5,?5)",
            params![chat_id, current_layout.project_id, title, position, now],
        )?;
        transaction.execute(
            "INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
             VALUES (?1,?2,?2,?3)",
            params![current_layout.project_id, chat_id, now],
        )?;
        require_one(
            transaction.execute(
                "UPDATE project_layouts SET active_tab_id=?2,updated_at=?3 WHERE project_id=?1",
                params![current_layout.project_id, chat_id, now],
            )?,
            "active Project layout disappeared during Chat creation",
        )?;
        require_one(
            transaction.execute(
                "UPDATE acp_project_usage
                 SET chat_count=chat_count+1,revision=revision+1
                 WHERE project_id=?1 AND chat_count=?2",
                params![current_layout.project_id, chat_count],
            )?,
            "Project Chat capacity changed during Chat creation",
        )?;
        require_one(
            transaction.execute(
                "UPDATE acp_store_usage
                 SET chat_count=chat_count+1,revision=revision+1
                 WHERE singleton=1 AND chat_count=?1",
                [store_chat_count],
            )?,
            "global Chat capacity changed during Chat creation",
        )?;
        transaction.commit()?;
        Ok(chat_id)
    }
}

fn require_one(changed: usize, message: &str) -> AppResult<()> {
    if changed != 1 {
        return Err(AppError::InvalidRequest(message.into()));
    }
    Ok(())
}
