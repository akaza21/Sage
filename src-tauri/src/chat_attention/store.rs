// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
// Upstream: packages/client-runtime/src/state/threadShell.ts; apps/web/src/components/Sidebar.logic.ts.
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::de::DeserializeOwned;

use super::{
    contract::{ChatAttentionV1, Position},
    snapshot::{self, Snapshot},
};
use crate::acp::{
    contract::{SessionState, TurnState},
    store::StoreError,
};

pub(crate) fn refresh(
    transaction: &Transaction<'_>,
    project: &str,
    chat: &str,
) -> Result<(), StoreError> {
    let next = snapshot::load(transaction, project, chat)?;
    let previous = transaction.query_row(
        "SELECT session_id,session_state,turn_state,terminal_turn_id,terminal_state,pending_approvals,pending_requests,request_identity
         FROM chat_attention WHERE chat_id=?1", [chat], |row| Ok(Snapshot {
            session_id: row.get(0)?, session_state: row.get(1)?, turn_state: row.get(2)?,
            terminal_turn_id: row.get(3)?, terminal_state: row.get(4)?, approvals: row.get(5)?, requests: row.get(6)?, request_identity: row.get(7)?,
        })).optional()?;
    if previous.as_ref() == Some(&next) {
        return Ok(());
    }
    let old = previous.unwrap_or_default();
    transaction.execute(
        "INSERT INTO chat_attention(chat_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [chat],
    )?;
    if old == next {
        return Ok(());
    }
    let (activity, revision): (i64, i64) = transaction.query_row(
        "SELECT activity_position,revision FROM chat_attention WHERE chat_id=?1",
        [chat],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let activity = increment(activity)?;
    let terminal_changed =
        next.terminal_turn_id != old.terminal_turn_id || next.terminal_state != old.terminal_state;
    transaction.execute("UPDATE chat_attention SET session_id=?2,session_state=?3,turn_state=?4,
        terminal_turn_id=?5,terminal_state=?6,pending_approvals=?7,pending_requests=?8,request_identity=?9,
        activity_position=?10,revision=?11,terminal_position=CASE WHEN ?12 THEN ?10 ELSE terminal_position END WHERE chat_id=?1",
        params![chat,next.session_id,next.session_state,next.turn_state,next.terminal_turn_id,next.terminal_state,
            next.approvals,next.requests,next.request_identity,activity,increment(revision)?,terminal_changed])?;
    if (next.approvals, next.requests, next.request_identity)
        != (old.approvals, old.requests, old.request_identity)
        && let Some(parent) = snapshot::side_parent(transaction, project, chat)?
    {
        invalidate(transaction, &parent)?;
    }
    Ok(())
}

fn invalidate(transaction: &Transaction<'_>, parent: &str) -> Result<(), StoreError> {
    transaction.execute(
        "INSERT INTO chat_attention(chat_id) VALUES(?1) ON CONFLICT DO NOTHING",
        [parent],
    )?;
    let (activity, revision): (i64, i64) = transaction.query_row(
        "SELECT activity_position,revision FROM chat_attention WHERE chat_id=?1",
        [parent],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    transaction.execute(
        "UPDATE chat_attention SET activity_position=?2,revision=?3 WHERE chat_id=?1",
        params![parent, increment(activity)?, increment(revision)?],
    )?;
    Ok(())
}

pub(crate) fn removing_side_chat(
    transaction: &Transaction<'_>,
    project: &str,
    chat: &str,
) -> Result<(), StoreError> {
    let summary = get_one(transaction, project, chat)?;
    if (summary.pending_approvals > 0 || summary.pending_requests > 0)
        && let Some(parent) = snapshot::side_parent(transaction, project, chat)?
    {
        invalidate(transaction, &parent)?;
    }
    Ok(())
}

pub(crate) fn get(
    connection: &Connection,
    project: &str,
    chats: &[String],
) -> Result<Vec<ChatAttentionV1>, StoreError> {
    if chats.len() > 64 {
        return Err(StoreError::Capacity {
            resource: "attentionIds",
            limit: 64,
        });
    }
    chats
        .iter()
        .map(|chat| get_one(connection, project, chat))
        .collect()
}

pub(super) fn get_one(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<ChatAttentionV1, StoreError> {
    let mut value = connection.query_row(
        "SELECT a.session_id,a.session_state,a.turn_state,COALESCE(a.pending_approvals,0),COALESCE(a.pending_requests,0),
         COALESCE(a.activity_position,0),a.read_position,a.terminal_position,a.terminal_state,a.forced_unread_position,COALESCE(a.revision,0)
         FROM chats c LEFT JOIN chat_attention a ON a.chat_id=c.id WHERE c.project_id=?1 AND c.id=?2",
        params![project,chat], |row| Ok((row.get::<_,Option<String>>(0)?, row.get::<_,Option<String>>(1)?,row.get::<_,Option<String>>(2)?,
            row.get::<_,u32>(3)?,row.get::<_,u32>(4)?,row.get::<_,i64>(5)?,row.get::<_,Option<i64>>(6)?,row.get::<_,Option<i64>>(7)?,
            row.get::<_,Option<String>>(8)?,row.get::<_,Option<i64>>(9)?,row.get::<_,i64>(10)?)))
        .optional()?.ok_or(StoreError::Conflict { resource: "attentionChat" })?;
    let mut statement = connection.prepare(
        "SELECT w.id,a.pending_approvals,a.pending_requests FROM workbench_tabs w
        JOIN chat_attention a ON a.chat_id=w.id WHERE w.project_id=?1 AND w.id GLOB 'side-*'
        AND json_extract(w.metadata_json,'$.parentChatId')=?2 LIMIT 9",
    )?;
    let children = statement
        .query_map(params![project, chat], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, u32>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if children.len() > 8 {
        return Err(StoreError::Corrupt("attention side chat bound"));
    }
    for (child, approvals, requests) in children {
        if snapshot::side_parent(connection, project, &child)?.as_deref() == Some(chat) {
            value.3 = value
                .3
                .checked_add(approvals)
                .ok_or(StoreError::CounterExhausted)?;
            value.4 = value
                .4
                .checked_add(requests)
                .ok_or(StoreError::CounterExhausted)?;
        }
    }
    let session_state: Option<SessionState> = parse(value.1)?;
    let turn_state: Option<TurnState> = parse(value.2)?;
    Ok(ChatAttentionV1 {
        project_id: crate::acp::contract::SageProjectId::parse(project)
            .map_err(|_| StoreError::Corrupt("attention project"))?,
        chat_id: crate::acp::contract::SageChatId::parse(chat)
            .map_err(|_| StoreError::Corrupt("attention chat"))?,
        session_id: value
            .0
            .map(crate::acp::contract::SageAgentSessionId::parse)
            .transpose()
            .map_err(|_| StoreError::Corrupt("attention session"))?,
        session_state,
        turn_state,
        running: matches!(
            session_state,
            Some(SessionState::Starting | SessionState::Recovering)
        ) || turn_state.is_some_and(|state| !state.is_terminal()),
        pending_approvals: value.3,
        pending_requests: value.4,
        activity_position: position(value.5)?,
        read_position: value.6.map(position).transpose()?,
        terminal_position: value.7.map(position).transpose()?,
        terminal_state: parse(value.8)?,
        forced_unread_position: value.9.map(position).transpose()?,
        revision: position(value.10)?,
        lineage: snapshot::lineage(connection, project, chat)?,
    })
}

fn parse<T: DeserializeOwned>(value: Option<String>) -> Result<Option<T>, StoreError> {
    value
        .map(|text| {
            serde_json::from_value(serde_json::Value::String(text))
                .map_err(|_| StoreError::Corrupt("attention lifecycle"))
        })
        .transpose()
}

pub(crate) fn position(value: i64) -> Result<Position, StoreError> {
    u64::try_from(value)
        .ok()
        .and_then(|value| Position::new(value).ok())
        .ok_or(StoreError::Corrupt("attention position"))
}

pub(super) fn increment(value: i64) -> Result<i64, StoreError> {
    value
        .checked_add(1)
        .filter(|value| *value >= 0)
        .ok_or(StoreError::CounterExhausted)
}

pub(crate) fn rail_status(summary: &ChatAttentionV1) -> &'static str {
    if summary.pending_approvals > 0 || summary.pending_requests > 0 {
        return "waiting";
    }
    if summary.running {
        if summary.session_state == Some(SessionState::Recovering) {
            return "recovering";
        }
        return if matches!(
            summary.turn_state,
            Some(TurnState::WaitingApproval | TurnState::WaitingUser)
        ) {
            "waiting"
        } else {
            "working"
        };
    }
    if matches!(
        summary.terminal_state,
        Some(TurnState::Failed | TurnState::DeliveryUnknown)
    ) || matches!(
        summary.session_state,
        Some(SessionState::Failed | SessionState::Incompatible)
    ) {
        return "failed";
    }
    if summary.terminal_state == Some(TurnState::Completed) {
        "complete"
    } else {
        "idle"
    }
}
