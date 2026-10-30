// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::contract::{AttentionLineage, AttentionRelation};
use crate::acp::store::StoreError;

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub session_id: Option<String>,
    pub session_state: Option<String>,
    pub turn_state: Option<String>,
    pub terminal_turn_id: Option<String>,
    pub terminal_state: Option<String>,
    pub approvals: u32,
    pub requests: u32,
    pub request_identity: String,
}

pub(super) fn load(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<Snapshot, StoreError> {
    let mut snapshot = connection.query_row(
        "SELECT id,state FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 ORDER BY ordinal DESC LIMIT 1",
        params![project,chat], |row| Ok(Snapshot { session_id: row.get(0)?, session_state: row.get(1)?, ..Snapshot::default() })
    ).optional()?.unwrap_or_default();
    if let Some(session) = &snapshot.session_id {
        snapshot.turn_state = connection
            .query_row(
                "SELECT state FROM turns WHERE session_id=?1 ORDER BY ordinal DESC LIMIT 1",
                [session],
                |row| row.get(0),
            )
            .optional()?;
    }
    if let Some((id, state)) = connection.query_row(
        "SELECT t.id,t.state FROM agent_sessions s JOIN turns t ON t.session_id=s.id
         WHERE s.project_id=?1 AND s.chat_id=?2 AND t.state IN ('completed','interrupted','cancelled','failed','deliveryUnknown')
         ORDER BY s.ordinal DESC,t.ordinal DESC LIMIT 1", params![project,chat],
        |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?))).optional()? {
        snapshot.terminal_turn_id = Some(id);
        snapshot.terminal_state = Some(state);
    }
    if matches!(
        snapshot.session_state.as_deref(),
        Some("failed" | "incompatible")
    ) && !matches!(
        snapshot.turn_state.as_deref(),
        Some("failed" | "deliveryUnknown")
    ) {
        snapshot.terminal_turn_id = snapshot
            .session_id
            .as_ref()
            .map(|session| format!("session:{session}"));
        snapshot.terminal_state = Some("failed".into());
    }
    let mut statement = connection.prepare(
        "SELECT id,normalized_payload_json FROM server_requests WHERE project_id=?1 AND chat_id=?2
         AND state IN ('received','asked') AND json_extract(normalized_payload_json,'$.kind')='permissionRequest' ORDER BY id LIMIT ?3")?;
    let maximum = crate::acp::limits::LIMITS.durable.requests_global;
    let mut rows = statement.query(params![project, chat, maximum as i64 + 1])?;
    let mut digest = Sha256::new();
    let mut count = 0;
    while let Some(row) = rows.next()? {
        count += 1;
        if count > maximum {
            return Err(StoreError::Corrupt("attention request bound"));
        }
        let payload: String = row.get(1)?;
        let normalized: crate::acp::intent::NormalizedPayload = serde_json::from_str(&payload)
            .map_err(|_| StoreError::Corrupt("attention normalized request"))?;
        if matches!(
            normalized,
            crate::acp::intent::NormalizedPayload::PermissionRequest { .. }
        ) {
            snapshot.approvals = snapshot
                .approvals
                .checked_add(1)
                .ok_or(StoreError::CounterExhausted)?;
            let id: String = row.get(0)?;
            digest.update((id.len() as u64).to_le_bytes());
            digest.update(id.as_bytes());
        }
    }
    if snapshot.approvals > 0 {
        snapshot.request_identity = format!("{:x}", digest.finalize());
    }
    Ok(snapshot)
}

pub(super) fn side_parent(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<Option<String>, StoreError> {
    let Some(suffix) = chat.strip_prefix("side-") else {
        return Ok(None);
    };
    if !uuid::Uuid::parse_str(suffix).is_ok_and(|id| id.to_string() == suffix) {
        return Ok(None);
    }
    Ok(connection
        .query_row(
            "SELECT p.id FROM workbench_tabs w JOIN chats p ON p.project_id=w.project_id
         AND p.id=json_extract(w.metadata_json,'$.parentChatId')
         WHERE w.project_id=?1 AND w.id=?2 AND w.kind='chat' AND p.id NOT GLOB 'side-*'",
            params![project, chat],
            |row| row.get(0),
        )
        .optional()?)
}

pub(super) fn lineage(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<Option<AttentionLineage>, StoreError> {
    let parent = if let Some(parent) = side_parent(connection, project, chat)? {
        Some((project.to_owned(), parent, AttentionRelation::SideChat))
    } else {
        connection
            .query_row(
                "SELECT parent_project_id,parent_chat_id FROM chat_lineage
            WHERE child_project_id=?1 AND child_chat_id=?2 AND relation='nativeFork'",
                params![project, chat],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        AttentionRelation::NativeFork,
                    ))
                },
            )
            .optional()?
    };
    parent
        .map(|(project, chat, relation)| {
            Ok(AttentionLineage {
                parent_project_id: crate::acp::contract::SageProjectId::parse(project)
                    .map_err(|_| StoreError::Corrupt("attention parent project"))?,
                parent_chat_id: crate::acp::contract::SageChatId::parse(chat)
                    .map_err(|_| StoreError::Corrupt("attention parent chat"))?,
                relation,
            })
        })
        .transpose()
}
