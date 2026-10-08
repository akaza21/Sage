use rusqlite::{OptionalExtension, params};
use serde::Deserialize;

use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    intent::SessionPromptContentV1,
};

use super::{StoreError, facts::hex_digest};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContentFact {
    runtime_epoch: u64,
    content: SessionPromptContentV1,
}

/// A missing, damaged, or previous-epoch fact never grants prompt content.
pub(crate) fn load_prompt_content(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
) -> Result<SessionPromptContentV1, StoreError> {
    Ok(recorded_prompt_content(connection, project, chat, session, epoch)?.unwrap_or_default())
}

pub(crate) fn recorded_prompt_content(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
) -> Result<Option<SessionPromptContentV1>, StoreError> {
    let row: Option<(String, i64, String)> = connection
        .query_row(
            "SELECT payload_json,payload_bytes,payload_digest FROM session_facts
             WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id IS NULL
               AND kind='sessionPromptContentV1' ORDER BY sequence DESC LIMIT 1",
            params![project.as_str(), chat.as_str(), session.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((payload, bytes, digest)) = row else {
        return Ok(None);
    };
    if usize::try_from(bytes).ok() != Some(payload.len())
        || hex_digest(payload.as_bytes()) != digest
    {
        return Ok(None);
    }
    let Ok(fact) = serde_json::from_str::<ContentFact>(&payload) else {
        return Ok(None);
    };
    if fact.runtime_epoch != epoch.get() || !fact.content.validate() {
        return Ok(None);
    }
    Ok(Some(fact.content))
}
