pub(crate) mod legacy_welcome;
mod store;

#[cfg(test)]
pub(crate) use store::seed_legacy_welcome;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{
    acp::limits::LIMITS,
    domain::{ChatMessage, SimulatorTurnResult},
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
};

#[cfg(test)]
use crate::harnesses::SIMULATOR_DEFAULT_MODEL_ID;

pub const MAX_BOOTSTRAP_MESSAGES: i64 = 500;
pub const MAX_BOOTSTRAP_MESSAGE_BYTES: i64 = 4 * 1024 * 1024;
pub const MAX_SESSION_EVENTS: i64 = 5_000;
pub const MAX_SESSION_EVENT_BYTES: i64 = 64 * 1024 * 1024;
pub const MAX_GLOBAL_EVENTS: i64 = 50_000;
pub const MAX_GLOBAL_EVENT_BYTES: i64 = 256 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessagePayload {
    role: String,
    text: String,
}

impl Database {
    #[cfg(test)]
    pub(crate) fn append_simulator_turn(
        &self,
        chat_id: &str,
        user_text: &str,
        assistant_text: &str,
    ) -> AppResult<SimulatorTurnResult> {
        self.append_simulator_turn_for_model(
            chat_id,
            SIMULATOR_DEFAULT_MODEL_ID,
            user_text,
            assistant_text,
        )
    }

    pub(crate) fn append_simulator_turn_for_model(
        &self,
        chat_id: &str,
        model_id: &str,
        user_text: &str,
        assistant_text: &str,
    ) -> AppResult<SimulatorTurnResult> {
        self.append_simulator_turn_inner(chat_id, model_id, user_text, assistant_text, || {})
    }

    #[cfg(test)]
    pub(crate) fn append_simulator_turn_with_hook(
        &self,
        chat_id: &str,
        user_text: &str,
        assistant_text: &str,
        after_first_insert: impl FnOnce(),
    ) -> AppResult<SimulatorTurnResult> {
        self.append_simulator_turn_for_model_with_hook(
            chat_id,
            SIMULATOR_DEFAULT_MODEL_ID,
            user_text,
            assistant_text,
            after_first_insert,
        )
    }

    #[cfg(test)]
    pub(crate) fn append_simulator_turn_for_model_with_hook(
        &self,
        chat_id: &str,
        model_id: &str,
        user_text: &str,
        assistant_text: &str,
        after_first_insert: impl FnOnce(),
    ) -> AppResult<SimulatorTurnResult> {
        self.append_simulator_turn_inner(
            chat_id,
            model_id,
            user_text,
            assistant_text,
            after_first_insert,
        )
    }

    fn append_simulator_turn_inner(
        &self,
        chat_id: &str,
        model_id: &str,
        user_text: &str,
        assistant_text: &str,
        after_first_insert: impl FnOnce(),
    ) -> AppResult<SimulatorTurnResult> {
        let payloads = [
            payload("user", user_text)?,
            payload("assistant", assistant_text)?,
        ];
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let session_id = store::append_turn(
            &transaction,
            chat_id,
            model_id,
            &payloads,
            now_millis(),
            after_first_insert,
        )?;
        let messages = messages_for_chat_in(&transaction, chat_id)?;
        transaction.commit()?;
        Ok(SimulatorTurnResult {
            session_id,
            messages,
        })
    }

    #[cfg(test)]
    pub(crate) fn messages(&self, session_id: &str) -> AppResult<Vec<ChatMessage>> {
        let chat_id: String = self.connection().query_row(
            "SELECT chat_id FROM agent_sessions WHERE id=?1",
            [session_id],
            |row| row.get(0),
        )?;
        self.messages_for_chat(&chat_id)
    }

    pub(crate) fn messages_for_chat(&self, chat_id: &str) -> AppResult<Vec<ChatMessage>> {
        let connection = self.connection();
        messages_for_chat_in(&connection, chat_id)
    }
}

fn messages_for_chat_in(
    connection: &rusqlite::Connection,
    chat_id: &str,
) -> AppResult<Vec<ChatMessage>> {
    verify_chat_item_ledgers(connection, chat_id)?;
    let welcome = legacy_welcome::turn_id(connection, chat_id)?.unwrap_or_default();
    let mut statement = connection.prepare(
        "WITH ordered AS (
               SELECT i.id,i.session_id,i.payload_json,i.created_at,
                      ROW_NUMBER() OVER (
                        ORDER BY s.ordinal,t.ordinal,t.id,i.ordinal,i.id
                      ) AS sequence
               FROM items i
               JOIN turns t ON t.id=i.turn_id
               JOIN agent_sessions s ON s.id=i.session_id
               WHERE i.chat_id=?1 AND i.kind='text' AND i.origin='primary'
                 AND (s.legacy=1 OR s.binding_id=?3)
                 AND t.id<>?4
             )
             SELECT id,session_id,sequence,payload_json,created_at FROM ordered
             ORDER BY sequence DESC LIMIT ?2",
    )?;
    let mut rows = statement.query(params![
        chat_id,
        MAX_BOOTSTRAP_MESSAGES,
        crate::harnesses::SIMULATOR_ID,
        welcome
    ])?;
    let mut messages = Vec::with_capacity(MAX_BOOTSTRAP_MESSAGES as usize);
    let mut bytes = 0_i64;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let session_id: String = row.get(1)?;
        let sequence: i64 = row.get(2)?;
        let payload: String = row.get(3)?;
        let created_at: i64 = row.get(4)?;
        let row_bytes = id.len() as i64 + session_id.len() as i64 + payload.len() as i64;
        if bytes.saturating_add(row_bytes) > MAX_BOOTSTRAP_MESSAGE_BYTES {
            break;
        }
        bytes += row_bytes;
        let payload: MessagePayload = serde_json::from_str(&payload)
            .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
        messages.push(ChatMessage {
            id,
            session_id,
            sequence,
            role: payload.role,
            text: payload.text,
            created_at,
        });
    }
    messages.reverse();
    Ok(messages)
}

fn payload(role: &str, text: &str) -> AppResult<String> {
    let payload = serde_json::to_string(&MessagePayload {
        role: role.into(),
        text: text.into(),
    })
    .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    if payload.len() > 262_144 {
        return Err(AppError::ResourceLimit(
            "simulator message exceeds the 256 KiB Item limit".into(),
        ));
    }
    Ok(payload)
}

fn verify_chat_item_ledgers(connection: &rusqlite::Connection, chat_id: &str) -> AppResult<()> {
    let session_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM agent_sessions WHERE chat_id=?1 LIMIT ?2
         )",
        params![chat_id, LIMITS.durable.sessions_per_chat as i64 + 1],
        |row| row.get(0),
    )?;
    if session_count > LIMITS.durable.sessions_per_chat as i64 {
        return Err(AppError::ResourceLimit(
            "simulator Chat exceeds its session-attempt limit".into(),
        ));
    }
    let mismatch: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM agent_sessions s
           LEFT JOIN acp_session_usage u ON u.session_id=s.id
           WHERE s.chat_id=?1 AND (
             u.session_id IS NULL OR u.item_count > ?2 OR
             u.item_count <> (
               SELECT COUNT(*) FROM (
                 SELECT 1 FROM items i WHERE i.session_id=s.id LIMIT ?3
               )
             )
           ) LIMIT 1
         )",
        params![chat_id, MAX_SESSION_EVENTS, MAX_SESSION_EVENTS + 1],
        |row| row.get(0),
    )?;
    if mismatch {
        return Err(AppError::InvalidRequest(
            "simulator Chat usage ledger does not match durable Items".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
