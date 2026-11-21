use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::harnesses::SIMULATOR_ID;

pub(crate) fn turn_id(
    connection: &rusqlite::Connection,
    chat: &str,
) -> rusqlite::Result<Option<String>> {
    let binding_digest = format!(
        "{:x}",
        Sha256::digest(format!("sage-simulator-binding-v1\0{SIMULATOR_ID}").as_bytes())
    );
    connection.query_row(
        "SELECT t.id FROM agent_sessions s
         JOIN chats c ON c.id=s.chat_id AND c.project_id=s.project_id
         JOIN acp_session_usage u ON u.session_id=s.id
         JOIN turns t ON t.session_id=s.id AND t.ordinal=0
         JOIN items i ON i.turn_id=t.id AND i.ordinal=0
         JOIN session_facts f ON f.session_id=s.id AND f.sequence=1 AND f.turn_id=t.id
         JOIN session_runtime_epochs e ON e.session_id=s.id AND e.runtime_epoch=1
         WHERE s.chat_id=?1 AND s.ordinal=0 AND s.binding_id=?2 AND s.binding_digest=?3
           AND s.external_session_id='sage-simulator:'||s.id AND s.model_id='sim-fast'
           AND s.state='closed' AND s.residency='cold' AND s.legacy=0 AND s.revision=0
           AND s.projection_revision=1 AND s.created_at=c.created_at
           AND s.updated_at=s.created_at AND s.terminal_at=s.created_at
           AND u.turn_count=1 AND u.item_count=1 AND u.fact_count=1 AND u.revision=0
           AND t.runtime_epoch=1 AND t.state='completed' AND t.delivery_state='correlated'
           AND t.client_intent_id='simulator-'||t.id AND t.revision=0
           AND t.created_at=s.created_at AND t.updated_at=t.created_at AND t.terminal_at=t.created_at
           AND i.origin='primary' AND i.kind='text' AND i.lifecycle='completed' AND i.revision=0
           AND json_extract(i.payload_json,'$.role')='assistant'
           AND i.created_at=t.created_at AND i.updated_at=i.created_at AND i.terminal_at=i.created_at
           AND f.kind='simulatorTurnCommitted' AND json_extract(f.payload_json,'$.itemCount')=1
           AND f.projection_revision=1 AND f.created_at=t.created_at
           AND e.state='exited' AND e.exit_kind='simulatorSynchronous'
           AND e.started_at=t.created_at AND e.terminal_at=t.created_at
           AND NOT EXISTS(SELECT 1 FROM chat_session_heads h WHERE h.session_id=s.id)
         LIMIT 1",
        params![chat, SIMULATOR_ID, binding_digest],
        |row| row.get(0),
    ).optional()
}
