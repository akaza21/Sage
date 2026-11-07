//! Which chats, across every project, need the user's attention: a finished
//! latest Turn the user has not seen, or a Turn waiting on an approval. The
//! rail shows this for projects that are not open, so it is answered from
//! durable truth: Turns in the project database and the read marks in the
//! settings store (settings_store/chat_read_marks.rs), which this module
//! only reads.

use std::collections::HashMap;

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::{
    error::AppResult,
    persistence::Database,
    project_registry::{CHAT_LATEST_TURN_ID, MAX_AGENT_RAIL_CHATS_PER_PROJECT, RAIL_RECENCY_ORDER},
    settings_store::SettingsStore,
};

/// The most chats one answer lists. Chats awaiting an approval come first,
/// then the most recently finished, so a cut never hides an approval behind
/// an unread chat.
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
    /// The latest Turn is waiting for the user to answer an approval.
    AwaitingApproval,
    /// The latest Turn finished (Done or Failed) after the chat's read mark,
    /// or the chat has none. `finished_at` is what reading it marks through.
    #[serde(rename_all = "camelCase")]
    Unread { finished_at: i64 },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttentionList {
    pub chats: Vec<ChatAttention>,
    /// More chats need attention than `chats` lists.
    pub truncated: bool,
}

/// The chats the rail previews (attached, not temporary, and among each
/// project's first `MAX_AGENT_RAIL_CHATS_PER_PROJECT` in its recency order)
/// whose latest Turn waits on an approval or has finished; approvals first,
/// then newest finish first. The read marks are bounded to exactly this many
/// chats, so a chat the user has read keeps its mark while it is listed here.
///
/// The ranking pass reads only tab and chat rows. The latest Turn is looked
/// up for the ranked-in chats alone, so at most `MAX_CHAT_READ_MARKS` rows
/// reach the final sort, however many chats the projects hold.
fn attention_candidates() -> String {
    format!(
        "SELECT c.project_id, c.tab_id, t.state = 'waitingApproval', t.terminal_at
         FROM (
           SELECT w.project_id AS project_id, w.id AS tab_id, c.id AS id,
                  ROW_NUMBER() OVER (PARTITION BY w.project_id ORDER BY {RAIL_RECENCY_ORDER}) AS rank
           FROM workbench_tabs w
           JOIN chat_tabs ct ON ct.project_id = w.project_id AND ct.tab_id = w.id
           JOIN chats c ON c.project_id = ct.project_id AND c.id = ct.chat_id
           WHERE w.kind = 'chat'
             AND COALESCE(json_extract(w.metadata_json, '$.ephemeral'), 0) = 0
         ) c
         JOIN turns t ON t.id = {CHAT_LATEST_TURN_ID}
         WHERE c.rank <= ?1
           AND (t.state = 'waitingApproval'
             OR (t.state IN ('completed', 'failed', 'deliveryUnknown')
                 AND t.terminal_at IS NOT NULL))
         ORDER BY t.state = 'waitingApproval' DESC, t.terminal_at DESC, c.project_id, c.tab_id"
    )
}

/// Lists the chats needing attention, given each chat's read mark. Memory
/// is bounded by the candidate cap above and the `MAX_CHAT_ATTENTION`
/// answer; nothing outlives the call.
pub(crate) fn chat_attention_in(
    connection: &Connection,
    read_through: &HashMap<String, i64>,
) -> AppResult<ChatAttentionList> {
    let mut statement = connection.prepare(&attention_candidates())?;
    let mut rows = statement.query(params![MAX_AGENT_RAIL_CHATS_PER_PROJECT])?;
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
        .into_iter()
        .map(|mark| (mark.chat_id, mark.read_through))
        .collect::<HashMap<_, _>>();
    chat_attention_in(&database.connection(), &marks)
}

#[cfg(test)]
mod tests;
