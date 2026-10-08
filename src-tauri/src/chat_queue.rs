use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    acp::attached_context::{AttachedContext, AttachedContexts},
    error::{AppError, AppResult},
    image_artifacts::verify_image_reference,
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
        let mut images = HashMap::new();
        for prompt in prompts {
            for context in prompt.attached_context.as_slice() {
                if let AttachedContext::Image(image) = context {
                    if images
                        .insert(image.artifact_id.clone(), (prompt.id.as_str(), image))
                        .is_some()
                    {
                        return Err(AppError::InvalidRequest(
                            "Image artifact occurs in multiple queued prompts".into(),
                        ));
                    }
                }
            }
        }
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
        if !images.is_empty() {
            let root = self.image_artifact_root()?;
            for (id, (_, image)) in &images {
                verify_image_reference(&transaction, root, project_id, chat_id, image)?;
                let state: String = transaction.query_row(
                    "SELECT lifecycle FROM chat_image_artifacts WHERE id=?1",
                    [id],
                    |row| row.get(0),
                )?;
                if !matches!(state.as_str(), "pending" | "queued") {
                    return Err(AppError::InvalidRequest(
                        "Image artifact cannot enter a queued prompt".into(),
                    ));
                }
            }
        }
        let mut queued_statement = transaction.prepare(
            "SELECT id FROM chat_image_artifacts WHERE project_id=?1 AND chat_id=?2 AND lifecycle='queued' ORDER BY id",
        )?;
        let queued_rows = queued_statement
            .query_map(params![project_id, chat_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(queued_statement);
        let mut retired = Vec::new();
        for id in queued_rows {
            if !images.contains_key(&id) {
                transaction.execute(
                    "UPDATE chat_image_artifacts SET lifecycle='retired',queue_id=NULL,updated_at=?2 WHERE id=?1",
                    params![id, now_millis()],
                )?;
                retired.push(id);
            }
        }
        for (id, (queue_id, _)) in &images {
            transaction.execute(
                "UPDATE chat_image_artifacts SET lifecycle='queued',queue_id=?2,updated_at=?3 WHERE id=?1",
                params![id, queue_id, now_millis()],
            )?;
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
        if !retired.is_empty() {
            let root = self.image_artifact_root()?;
            for id in retired {
                let _ = crate::acp::migration::remove_image_file(&root.join(format!("{id}.full")));
            }
        }
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
        let mut expected_images = HashSet::new();
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
            for prompt in &prompts {
                for context in prompt.attached_context.as_slice() {
                    if let AttachedContext::Image(image) = context {
                        if !expected_images.insert(image.artifact_id.clone()) {
                            return Err(AppError::InvalidRequest(
                                "Queued image artifact is duplicated".into(),
                            ));
                        }
                        verify_image_reference(
                            &connection,
                            self.image_artifact_root()?,
                            project_id,
                            &chat_id,
                            image,
                        )?;
                        let identity: (String, String) = connection.query_row(
                            "SELECT lifecycle,queue_id FROM chat_image_artifacts WHERE id=?1",
                            [&image.artifact_id],
                            |row| Ok((row.get(0)?, row.get(1)?)),
                        )?;
                        if identity.0 != "queued" || identity.1 != prompt.id {
                            return Err(AppError::InvalidRequest(
                                "Queued image artifact has a mismatched queue identity".into(),
                            ));
                        }
                    }
                }
            }
            for prompt in &mut prompts {
                if !matches!(prompt.status, QueuedPromptStatus::OutcomeUnknown) {
                    prompt.status = QueuedPromptStatus::Blocked;
                }
            }
            result.insert(chat_id, prompts);
        }
        drop(rows);
        drop(statement);
        let mut queued_statement = connection.prepare(
            "SELECT id FROM chat_image_artifacts WHERE project_id=?1 AND lifecycle='queued' ORDER BY id",
        )?;
        let queued_rows = queued_statement
            .query_map([project_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if queued_rows.len() != expected_images.len()
            || queued_rows.iter().any(|id| !expected_images.contains(id))
        {
            return Err(AppError::InvalidRequest(
                "Queued image artifacts disagree with durable Chat prompts".into(),
            ));
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "chat_queue_tests.rs"]
mod tests;
