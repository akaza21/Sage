use std::collections::HashMap;

use rusqlite::Connection;
use sage_core::error::AppResult;
use serde::Serialize;

use crate::{
    persistence::Database,
    project_registry::{CHAT_LATEST_TURN_ID, rail_chat_ids, rail_chat_set},
    settings_store::{ChatReadMark, ChatReadMarkWrite, SettingsStore},
};

/// Approvals sort first, so truncation never hides one behind an unread chat.
pub const MAX_CHAT_ATTENTION: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttention {
    pub project_id: String,
    pub chat_id: String,
    #[serde(flatten)]
    pub reason: AttentionReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "camelCase")]
pub enum AttentionReason {
    AwaitingApproval,
    /// Finished after the read mark, or with none; reading marks through
    /// `finished_at`.
    #[serde(rename_all = "camelCase")]
    Unread {
        finished_at: i64,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttentionList {
    pub chats: Vec<ChatAttention>,
    pub truncated: bool,
}

/// Rail chats whose latest Turn waits on an approval or has finished,
/// approvals first, then newest finish first. The read marks retain exactly
/// these chats (see `mark_read`), so a
/// chat the user has read keeps its mark while it is listed here.
/// Only rail chats are looked up, so at most `MAX_RAIL_CHATS` rows reach the sort.
fn attention_candidates() -> String {
    format!(
        "SELECT c.project_id, c.tab_id, t.state = 'waitingApproval', t.terminal_at
         FROM {} c
         JOIN turns t ON t.id = {CHAT_LATEST_TURN_ID}
         WHERE t.state = 'waitingApproval'
            OR (t.state IN ('completed', 'failed', 'deliveryUnknown')
                AND t.terminal_at IS NOT NULL)
         ORDER BY t.state = 'waitingApproval' DESC, t.terminal_at DESC, c.project_id, c.tab_id",
        rail_chat_set()
    )
}

pub(crate) fn chat_attention_in(
    connection: &Connection,
    read_through: &HashMap<String, i64>,
) -> AppResult<ChatAttentionList> {
    let mut statement = connection.prepare(&attention_candidates())?;
    let mut rows = statement.query([])?;
    let mut list = ChatAttentionList::default();
    while let Some(row) = rows.next()? {
        let chat_id: String = row.get(1)?;
        let reason = if row.get::<_, bool>(2)? {
            AttentionReason::AwaitingApproval
        } else {
            let finished_at: i64 = row.get(3)?;
            if read_through
                .get(&chat_id)
                .is_some_and(|mark| finished_at <= *mark)
            {
                continue;
            }
            AttentionReason::Unread { finished_at }
        };
        if list.chats.len() == MAX_CHAT_ATTENTION {
            list.truncated = true;
            break;
        }
        list.chats.push(ChatAttention {
            project_id: row.get(0)?,
            chat_id,
            reason,
        });
    }
    Ok(list)
}

/// The cross-project attention answer from the two stores. The read marks
/// are loaded first and the settings lock released before the project
/// database is locked, so the two locks are never held together.
pub(crate) fn chat_attention(
    database: &Database,
    settings: &SettingsStore,
) -> AppResult<ChatAttentionList> {
    let marks = settings
        .chat_read_marks()?
        .marks
        .into_iter()
        .map(|mark| (mark.chat_id, mark.read_through))
        .collect::<HashMap<_, _>>();
    chat_attention_in(&database.connection(), &marks)
}

/// Records a read mark. The rail's chats are read first and the project
/// database released before the settings store is locked, so the two locks
/// are never held together.
pub(crate) fn mark_read(
    database: &Database,
    settings: &SettingsStore,
    mark: &ChatReadMark,
) -> AppResult<ChatReadMarkWrite> {
    let retained = rail_chat_ids(&database.connection())?;
    settings.mark_chat_read(mark, &retained)
}

#[cfg(test)]
mod tests;
