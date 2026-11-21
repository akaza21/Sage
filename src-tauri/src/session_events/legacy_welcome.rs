use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};

use crate::harnesses::SIMULATOR_ID;

pub(crate) fn turn_id(
    connection: &rusqlite::Connection,
    chat: &str,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(&turn_query("?1"), [chat], |row| row.get(0))
        .optional()
}

/// The exact historical-welcome recognition shared with discovery. `chat` is
/// an internal SQL column or parameter, never text supplied by the WebView.
/// Only ordinal-zero rows are inspected, through their existing indexes.
pub(crate) fn turn_query(chat: &str) -> String {
    let binding_digest = format!(
        "{:x}",
        Sha256::digest(format!("sage-simulator-binding-v1\0{SIMULATOR_ID}").as_bytes())
    );
    format!(
        "SELECT welcome_turn.id FROM agent_sessions welcome_session
         JOIN chats welcome_chat ON welcome_chat.id=welcome_session.chat_id AND welcome_chat.project_id=welcome_session.project_id
         JOIN acp_session_usage welcome_usage ON welcome_usage.session_id=welcome_session.id
         JOIN turns welcome_turn ON welcome_turn.session_id=welcome_session.id AND welcome_turn.ordinal=0
         JOIN items welcome_item ON welcome_item.turn_id=welcome_turn.id AND welcome_item.ordinal=0
         JOIN session_facts welcome_fact ON welcome_fact.session_id=welcome_session.id AND welcome_fact.sequence=1 AND welcome_fact.turn_id=welcome_turn.id
         JOIN session_runtime_epochs welcome_epoch ON welcome_epoch.session_id=welcome_session.id AND welcome_epoch.runtime_epoch=1
         WHERE welcome_session.chat_id={chat} AND welcome_session.ordinal=0 AND welcome_session.binding_id='{SIMULATOR_ID}' AND welcome_session.binding_digest='{binding_digest}'
           AND welcome_session.external_session_id='sage-simulator:'||welcome_session.id AND welcome_session.model_id='sim-fast'
           AND welcome_session.state='closed' AND welcome_session.residency='cold' AND welcome_session.legacy=0 AND welcome_session.revision=0
           AND welcome_session.projection_revision=1 AND welcome_session.created_at=welcome_chat.created_at
           AND welcome_session.updated_at=welcome_session.created_at AND welcome_session.terminal_at=welcome_session.created_at
           AND welcome_usage.turn_count=1 AND welcome_usage.item_count=1 AND welcome_usage.fact_count=1 AND welcome_usage.revision=0
           AND welcome_turn.runtime_epoch=1 AND welcome_turn.state='completed' AND welcome_turn.delivery_state='correlated'
           AND welcome_turn.client_intent_id='simulator-'||welcome_turn.id AND welcome_turn.revision=0
           AND welcome_turn.created_at=welcome_session.created_at AND welcome_turn.updated_at=welcome_turn.created_at AND welcome_turn.terminal_at=welcome_turn.created_at
           AND welcome_item.origin='primary' AND welcome_item.kind='text' AND welcome_item.lifecycle='completed' AND welcome_item.revision=0
           AND json_extract(welcome_item.payload_json,'$.role')='assistant'
           AND welcome_item.created_at=welcome_turn.created_at AND welcome_item.updated_at=welcome_item.created_at AND welcome_item.terminal_at=welcome_item.created_at
           AND welcome_fact.kind='simulatorTurnCommitted' AND json_extract(welcome_fact.payload_json,'$.itemCount')=1
           AND welcome_fact.projection_revision=1 AND welcome_fact.created_at=welcome_turn.created_at
           AND welcome_epoch.state='exited' AND welcome_epoch.exit_kind='simulatorSynchronous'
           AND welcome_epoch.started_at=welcome_turn.created_at AND welcome_epoch.terminal_at=welcome_turn.created_at
           AND NOT EXISTS(SELECT 1 FROM chat_session_heads welcome_head WHERE welcome_head.session_id=welcome_session.id)
         LIMIT 1"
    )
}
