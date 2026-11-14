use rusqlite::params;
use uuid::Uuid;

use super::{
    ChatTitleSource, SIDE_CHAT_ID_PREFIX, placeholder_chat_title, require_one,
    validate_chat_identifier,
};
use crate::{
    acp::limits::LIMITS,
    domain::ProjectLayoutRequest,
    persistence::Database,
    workbench_persistence::{MAX_RESTORED_TABS, persist_layout_in},
};
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};

impl Database {
    #[cfg(test)]
    pub(crate) fn create_chat(&self, current_layout: &ProjectLayoutRequest) -> AppResult<String> {
        self.create_chat_on(&current_layout.project_id, current_layout)
    }

    #[cfg(test)]
    pub(crate) fn create_chat_on(
        &self,
        project_id: &str,
        current_layout: &ProjectLayoutRequest,
    ) -> AppResult<String> {
        self.create_chat_with_parent(project_id, current_layout, None)
    }

    pub(crate) fn create_chat_with_parent(
        &self,
        project_id: &str,
        current_layout: &ProjectLayoutRequest,
        parent: Option<&str>,
    ) -> AppResult<String> {
        self.create_chat_with_identity(project_id, current_layout, parent, None)
    }

    pub(crate) fn create_chat_with_identity(
        &self,
        project_id: &str,
        current_layout: &ProjectLayoutRequest,
        parent: Option<&str>,
        chat_id: Option<&str>,
    ) -> AppResult<String> {
        validate_chat_identifier("Project ID", project_id)?;
        if let Some(chat_id) = chat_id {
            Uuid::parse_str(chat_id)
                .map_err(|_| AppError::InvalidRequest("Chat creation ID must be a UUID".into()))?;
            if parent.is_some() {
                return Err(AppError::InvalidRequest(
                    "Side Chat identity is assigned natively".into(),
                ));
            }
        }
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        if let Some(parent) = parent {
            crate::side_chat_storage::validate_parent(&transaction, project_id, parent)?;
        }
        let active_project_id = transaction.query_row(
            "SELECT active_project_id FROM app_state WHERE singleton=1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )?;
        let source_is_active = active_project_id.as_deref() == Some(project_id);
        if source_is_active {
            if current_layout.tabs.len() >= MAX_RESTORED_TABS {
                return Err(AppError::ResourceLimit(format!(
                    "project layout has reached its {MAX_RESTORED_TABS}-tab limit"
                )));
            }
            if current_layout.project_id != project_id {
                return Err(AppError::InvalidRequest(
                    "Chat layout does not belong to the requested Project".into(),
                ));
            }
            persist_layout_in(&transaction, current_layout)?;
        } else {
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
                [project_id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(AppError::InvalidRequest(
                    "Chat does not belong to that Project".into(),
                ));
            }
            let tab_count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM workbench_tabs WHERE project_id=?1",
                [project_id],
                |row| row.get(0),
            )?;
            if tab_count >= MAX_RESTORED_TABS as i64 {
                return Err(AppError::ResourceLimit(format!(
                    "project layout has reached its {MAX_RESTORED_TABS}-tab limit"
                )));
            }
        }
        let chat_id =
            insert_chat_with_identity(&transaction, project_id, source_is_active, parent, chat_id)?;
        transaction.commit()?;
        Ok(chat_id)
    }
}

pub(crate) fn insert_new_chat(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source_is_active: bool,
    parent: Option<&str>,
) -> AppResult<String> {
    insert_chat_with_identity(transaction, project_id, source_is_active, parent, None)
}

fn insert_chat_with_identity(
    transaction: &rusqlite::Transaction<'_>,
    project_id: &str,
    source_is_active: bool,
    parent: Option<&str>,
    chat_id: Option<&str>,
) -> AppResult<String> {
    let chat_count: i64 = transaction.query_row(
        "SELECT chat_count FROM acp_project_usage WHERE project_id=?1",
        [project_id],
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
        [project_id],
        |row| row.get(0),
    )?;
    let position: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(position),-1)+1 FROM workbench_tabs WHERE project_id=?1",
        [project_id],
        |row| row.get(0),
    )?;
    let chat_id = if parent.is_some() {
        format!("{SIDE_CHAT_ID_PREFIX}{}", Uuid::new_v4())
    } else {
        chat_id
            .map(str::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string())
    };
    let title = placeholder_chat_title(&chat_id);
    let now = now_millis();
    transaction.execute(
        "INSERT INTO chats(
           id,project_id,ordinal,title,projection_revision,created_at,updated_at,title_source
         ) VALUES (?1,?2,?3,?4,0,?5,?5,?6)",
        params![
            chat_id,
            project_id,
            ordinal,
            title,
            now,
            ChatTitleSource::Default.as_str()
        ],
    )?;
    transaction.execute(
        "INSERT INTO workbench_tabs(
           id,project_id,kind,title,position,pinned,dirty,status,
           metadata_json,created_at,updated_at
         ) VALUES (?1,?2,'chat',?3,?4,0,0,'idle','{}',?5,?5)",
        params![chat_id, project_id, title, position, now],
    )?;
    transaction.execute(
        "INSERT INTO chat_tabs(project_id,chat_id,tab_id,attached_at)
         VALUES (?1,?2,?2,?3)",
        params![project_id, chat_id, now],
    )?;
    require_one(
        transaction.execute(
            "UPDATE project_layouts SET active_tab_id=?2,updated_at=?3 WHERE project_id=?1",
            params![project_id, chat_id, now],
        )?,
        if source_is_active {
            "active Project layout disappeared during Chat creation"
        } else {
            "Project layout disappeared during Chat creation"
        },
    )?;
    require_one(
        transaction.execute(
            "UPDATE acp_project_usage
             SET chat_count=chat_count+1,revision=revision+1
             WHERE project_id=?1 AND chat_count=?2",
            params![project_id, chat_count],
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
    if let Some(parent) = parent {
        transaction.execute(
            "UPDATE workbench_tabs SET metadata_json=?2 WHERE id=?1",
            params![
                chat_id,
                serde_json::json!({"ephemeral":true,"parentChatId":parent}).to_string()
            ],
        )?;
    }
    Ok(chat_id)
}
