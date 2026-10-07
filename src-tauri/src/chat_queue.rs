use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    acp::attached_context::AttachedContexts,
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
};

const MAX_PROMPTS: usize = 8;
const MAX_PROMPT_BYTES: usize = 16 * 1024;
const MAX_QUEUE_BYTES: usize = 512 * 1024;
const MAX_ID_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DurableQueuedPrompt {
    pub id: String,
    pub text: String,
    pub attached_context: AttachedContexts,
    pub binding_id: String,
    pub model_id: Option<String>,
    pub status: QueuedPromptStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum QueuedPromptStatus {
    Queued,
    Sending,
    Blocked,
    OutcomeUnknown,
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_BYTES && !value.contains('\0')
}

fn admitted_prompts(prompts: &[DurableQueuedPrompt]) -> AppResult<String> {
    if prompts.len() > MAX_PROMPTS {
        return Err(AppError::ResourceLimit(
            "Chat prompt queue exceeds eight items".into(),
        ));
    }
    let mut ids = std::collections::HashSet::new();
    for prompt in prompts {
        if !valid_id(&prompt.id)
            || !valid_id(&prompt.binding_id)
            || prompt
                .model_id
                .as_deref()
                .is_some_and(|value| !valid_id(value))
            || prompt.text.trim().is_empty()
            || prompt.text.len() > MAX_PROMPT_BYTES
            || !ids.insert(prompt.id.as_str())
        {
            return Err(AppError::InvalidRequest(
                "Queued Chat prompt is invalid".into(),
            ));
        }
    }
    let json = serde_json::to_string(prompts)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    if json.len() > MAX_QUEUE_BYTES {
        return Err(AppError::ResourceLimit(
            "Chat prompt queue exceeds 512 KiB".into(),
        ));
    }
    Ok(json)
}

impl Database {
    pub(crate) fn replace_chat_queued_prompts(
        &self,
        project_id: &str,
        chat_id: &str,
        prompts: &[DurableQueuedPrompt],
    ) -> AppResult<()> {
        if !valid_id(project_id) || !valid_id(chat_id) {
            return Err(AppError::InvalidRequest(
                "Queued Chat owner is invalid".into(),
            ));
        }
        let json = admitted_prompts(prompts)?;
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let belongs: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM chats WHERE project_id=?1 AND id=?2",
                params![project_id, chat_id],
                |row| row.get(0),
            )
            .optional()?;
        if belongs.is_none() {
            return Err(AppError::InvalidRequest(
                "Queued Chat owner is absent".into(),
            ));
        }
        if prompts.is_empty() {
            transaction.execute(
                "DELETE FROM chat_queued_prompts WHERE project_id=?1 AND chat_id=?2",
                params![project_id, chat_id],
            )?;
        } else {
            transaction.execute(
                "INSERT INTO chat_queued_prompts(project_id, chat_id, prompts_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(chat_id) DO UPDATE SET
                   prompts_json=excluded.prompts_json,
                   updated_at=excluded.updated_at",
                params![project_id, chat_id, json, now_millis()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn project_chat_queued_prompts(
        &self,
        project_id: &str,
    ) -> AppResult<HashMap<String, Vec<DurableQueuedPrompt>>> {
        if !valid_id(project_id) {
            return Err(AppError::InvalidRequest(
                "Queued Chat Project is invalid".into(),
            ));
        }
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT chat_id, prompts_json FROM chat_queued_prompts
             WHERE project_id=?1 ORDER BY chat_id LIMIT 129",
        )?;
        let mut rows = statement.query([project_id])?;
        let mut result = HashMap::new();
        while let Some(row) = rows.next()? {
            if result.len() >= 128 {
                return Err(AppError::ResourceLimit("Too many Chat queues".into()));
            }
            let chat_id: String = row.get(0)?;
            let json: String = row.get(1)?;
            if json.len() > MAX_QUEUE_BYTES {
                return Err(AppError::ResourceLimit(
                    "Chat prompt queue exceeds 512 KiB".into(),
                ));
            }
            let mut prompts: Vec<DurableQueuedPrompt> = serde_json::from_str(&json)
                .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
            admitted_prompts(&prompts)?;
            for prompt in &mut prompts {
                if !matches!(prompt.status, QueuedPromptStatus::OutcomeUnknown) {
                    prompt.status = QueuedPromptStatus::Blocked;
                }
            }
            result.insert(chat_id, prompts);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_survives_database_reopen_but_requires_explicit_retry() {
        let directory =
            std::env::temp_dir().join(format!("sage-chat-queue-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("sage.db");
        {
            let database = Database::open(&path).unwrap();
            let connection = database.connection();
            connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES ('p','P',1,1,1)", []).unwrap();
            connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES ('c','p',0,'C',1,1)", []).unwrap();
            drop(connection);
            database
                .replace_chat_queued_prompts(
                    "p",
                    "c",
                    &[DurableQueuedPrompt {
                        id: "q1".into(),
                        text: "later".into(),
                        attached_context: Default::default(),
                        binding_id: "binding".into(),
                        model_id: None,
                        status: QueuedPromptStatus::Queued,
                    }],
                )
                .unwrap();
        }
        {
            let database = Database::open(&path).unwrap();
            let restored = database.project_chat_queued_prompts("p").unwrap();
            let queue = &restored["c"];
            assert_eq!(queue.len(), 1);
            assert_eq!(queue[0].text, "later");
            assert!(matches!(queue[0].status, QueuedPromptStatus::Blocked));
            database.replace_chat_queued_prompts("p", "c", &[]).unwrap();
            assert!(
                database
                    .project_chat_queued_prompts("p")
                    .unwrap()
                    .is_empty()
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn outcome_unknown_queue_status_survives_database_reopen() {
        let directory =
            std::env::temp_dir().join(format!("sage-chat-queue-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("sage.db");
        {
            let database = Database::open(&path).unwrap();
            let connection = database.connection();
            connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES ('p','P',1,1,1)", []).unwrap();
            connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES ('c','p',0,'C',1,1)", []).unwrap();
            drop(connection);
            database
                .replace_chat_queued_prompts(
                    "p",
                    "c",
                    &[DurableQueuedPrompt {
                        id: "same-client-id".into(),
                        text: "maybe delivered".into(),
                        attached_context: Default::default(),
                        binding_id: "binding".into(),
                        model_id: None,
                        status: QueuedPromptStatus::Queued,
                    }],
                )
                .unwrap();
            database.connection().execute("UPDATE chat_queued_prompts SET prompts_json=json_set(prompts_json,'$[0].status','outcomeUnknown') WHERE chat_id='c'", []).unwrap();
        }
        let database = Database::open(&path).unwrap();
        let restored = database.project_chat_queued_prompts("p");
        assert!(restored.is_ok());
        let queue = &restored.unwrap()["c"];
        assert_eq!(queue[0].id, "same-client-id");
        assert_eq!(
            serde_json::to_value(queue[0].status).unwrap(),
            serde_json::json!("outcomeUnknown")
        );
        drop(database);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
