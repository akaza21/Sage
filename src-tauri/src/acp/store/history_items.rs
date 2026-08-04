use std::collections::BTreeMap;

use rusqlite::params;

use crate::acp::{
    contract::{
        BoundedVec, EntityRevision, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
        SageRootId, SageServerRequestId, SageTurnId,
    },
    intent::PageCursor,
    projection::{ActivityProjection, ItemProjection, TurnProjection},
};

use super::{
    StoreError,
    history::TurnRow,
    history_cursor,
    projection_decode::{ItemRow, decode_item},
};

const CURSOR_JSON_RESERVE: usize = 514;

pub(super) struct HistorySlice {
    pub(super) turns: Vec<TurnProjection>,
    pub(super) next_cursor: Option<PageCursor>,
}

pub(super) struct HistoryLoad<'a> {
    pub(super) connection: &'a rusqlite::Connection,
    pub(super) project: &'a SageProjectId,
    pub(super) chat: &'a SageChatId,
    pub(super) rows: &'a [TurnRow],
    pub(super) has_more_turns: bool,
    pub(super) project_roots: &'a [SageRootId],
    pub(super) activities: BTreeMap<String, Vec<ActivityProjection>>,
    pub(super) base_page_bytes: usize,
}

pub(super) fn load(input: HistoryLoad<'_>) -> Result<HistorySlice, StoreError> {
    let HistoryLoad {
        connection,
        project,
        chat,
        rows,
        has_more_turns,
        project_roots,
        mut activities,
        base_page_bytes,
    } = input;
    let limit = crate::acp::limits::LIMITS.durable.history_page_bytes as usize;
    let mut used = base_page_bytes
        .checked_add(CURSOR_JSON_RESERVE)
        .ok_or(StoreError::Corrupt("history page bytes"))?;
    let mut item_count = 0_usize;
    let mut turns: Vec<TurnProjection> = Vec::new();
    let mut next_cursor = None;

    for row in rows {
        if item_count == crate::acp::limits::LIMITS.durable.history_page_items as usize {
            let previous = turns.last().ok_or(StoreError::Capacity {
                resource: "historyPage",
                limit: limit as u64,
            })?;
            let previous_row = rows
                .iter()
                .find(|candidate| candidate.id == previous.id.as_str())
                .ok_or(StoreError::Corrupt("history cursor boundary"))?;
            next_cursor = Some(history_cursor::after_turn(project, chat, previous_row)?);
            break;
        }
        let activity = activities.remove(&row.id).unwrap_or_default();
        let mut turn = project_turn(row, Vec::new(), activity)?;
        let turn_bytes = serde_json::to_vec(&turn)
            .map_err(|_| StoreError::Corrupt("turn projection serialization"))?
            .len();
        let turn_delta = turn_bytes + usize::from(!turns.is_empty());
        if used.saturating_add(turn_delta) > limit {
            let previous = turns.last().ok_or(StoreError::Capacity {
                resource: "historyPage",
                limit: limit as u64,
            })?;
            let previous_row = rows
                .iter()
                .find(|candidate| candidate.id == previous.id.as_str())
                .ok_or(StoreError::Corrupt("history cursor boundary"))?;
            next_cursor = Some(history_cursor::after_turn(project, chat, previous_row)?);
            break;
        }
        used += turn_delta;
        let loaded = load_turn_items(
            connection,
            row,
            project_roots,
            limit,
            &mut used,
            &mut item_count,
        )?;
        turn.items = BoundedVec::new("historyPageItems", loaded.items)
            .map_err(|_| StoreError::Corrupt("item page bound"))?;
        turns.push(turn);
        if let Some(before) = loaded.more_before {
            next_cursor = Some(history_cursor::before_item(project, chat, row, before)?);
            break;
        }
    }
    if next_cursor.is_none() && (has_more_turns || turns.len() < rows.len()) {
        let last = turns
            .last()
            .ok_or(StoreError::Corrupt("empty history page"))?;
        let row = rows
            .iter()
            .find(|candidate| candidate.id == last.id.as_str())
            .ok_or(StoreError::Corrupt("history cursor boundary"))?;
        next_cursor = Some(history_cursor::after_turn(project, chat, row)?);
    }
    Ok(HistorySlice { turns, next_cursor })
}

struct LoadedItems {
    items: Vec<ItemProjection>,
    more_before: Option<i64>,
}

fn load_turn_items(
    connection: &rusqlite::Connection,
    turn: &TurnRow,
    project_roots: &[SageRootId],
    page_limit: usize,
    used: &mut usize,
    page_item_count: &mut usize,
) -> Result<LoadedItems, StoreError> {
    let remaining = crate::acp::limits::LIMITS.durable.history_page_items as usize
        - (*page_item_count).min(crate::acp::limits::LIMITS.durable.history_page_items as usize);
    let query_limit = remaining.saturating_add(1).max(1) as i64;
    let mut statement = connection.prepare(
        "SELECT i.id,i.project_id,i.chat_id,i.session_id,i.turn_id,i.runtime_epoch,
                i.ordinal,i.origin,i.activity_id,i.kind,i.lifecycle,i.tool_state,
                i.external_item_id,i.payload_json,i.payload_bytes,i.payload_digest,
                i.revision,i.created_at,i.updated_at,i.terminal_at,
                r.id,r.state,r.normalized_payload_json
         FROM items i
         JOIN agent_sessions s ON s.id=i.session_id
         LEFT JOIN server_requests r
           ON i.kind='permissionRequest' AND r.kind='permission'
          AND r.project_id=i.project_id AND r.chat_id=i.chat_id
          AND r.session_id=i.session_id AND r.turn_id=i.turn_id
          AND r.runtime_epoch=i.runtime_epoch
          AND r.binding_digest=s.binding_digest
          AND r.external_request_id=i.external_item_id
         WHERE i.project_id=?1 AND i.chat_id=?2 AND i.session_id=?3 AND i.turn_id=?4
           AND (?5 IS NULL OR i.ordinal<?5)
         ORDER BY i.ordinal DESC,i.id DESC LIMIT ?6",
    )?;
    let mut rows = statement.query(params![
        turn.project_id,
        turn.chat_id,
        turn.session_id,
        turn.id,
        turn.item_before,
        query_limit,
    ])?;
    let mut items = Vec::with_capacity(remaining.min(256));
    let mut previous_id: Option<String> = None;
    let mut last_accepted_ordinal = None;
    let mut more = false;
    while let Some(row) = rows.next()? {
        let item = ItemRow {
            id: row.get(0)?,
            project_id: row.get(1)?,
            chat_id: row.get(2)?,
            session_id: row.get(3)?,
            turn_id: row.get(4)?,
            runtime_epoch: row.get(5)?,
            ordinal: row.get(6)?,
            origin: row.get(7)?,
            activity_id: row.get(8)?,
            kind: row.get(9)?,
            lifecycle: row.get(10)?,
            tool_state: row.get(11)?,
            external_item_id: row.get(12)?,
            payload_json: row.get(13)?,
            payload_bytes: row.get(14)?,
            payload_digest: row.get(15)?,
            revision: row.get(16)?,
            created_at: row.get(17)?,
            updated_at: row.get(18)?,
            terminal_at: row.get(19)?,
        };
        if previous_id.as_deref() == Some(item.id.as_str()) {
            return Err(StoreError::Corrupt("duplicate permission item link"));
        }
        previous_id = Some(item.id.clone());
        if items.len() == remaining {
            more = true;
            break;
        }
        let permission = permission(row, &item)?;
        let projection = decode_item(item, permission, project_roots)?;
        let item_bytes = serde_json::to_vec(&projection)
            .map_err(|_| StoreError::Corrupt("item projection serialization"))?
            .len()
            + usize::from(!items.is_empty());
        if used.saturating_add(item_bytes) > page_limit {
            more = true;
            break;
        }
        *used += item_bytes;
        *page_item_count += 1;
        last_accepted_ordinal = Some(projection.ordinal as i64);
        items.push(projection);
    }
    if more && items.is_empty() {
        return Err(StoreError::Capacity {
            resource: "historyPage",
            limit: page_limit as u64,
        });
    }
    let more_before =
        more.then_some(last_accepted_ordinal.ok_or(StoreError::Corrupt("item cursor boundary"))?);
    items.reverse();
    Ok(LoadedItems { items, more_before })
}

fn permission(
    row: &rusqlite::Row<'_>,
    item: &ItemRow,
) -> Result<
    Option<(
        SageServerRequestId,
        crate::acp::lifecycle::ServerRequestState,
    )>,
    StoreError,
> {
    let request_id = row.get::<_, Option<String>>(20)?;
    let state = row.get::<_, Option<String>>(21)?;
    let normalized = row.get::<_, Option<String>>(22)?;
    match (request_id, state, normalized) {
        (None, None, None) => Ok(None),
        (Some(id), Some(state), Some(normalized)) if normalized == item.payload_json => Ok(Some((
            SageServerRequestId::parse(id).map_err(|_| StoreError::Corrupt("request id"))?,
            super::state::parse_request_state(&state)?,
        ))),
        _ => Err(StoreError::Corrupt("permission request item link")),
    }
}

fn project_turn(
    row: &TurnRow,
    items: Vec<ItemProjection>,
    activities: Vec<ActivityProjection>,
) -> Result<TurnProjection, StoreError> {
    Ok(TurnProjection {
        id: SageTurnId::parse(row.id.clone()).map_err(|_| StoreError::Corrupt("turn id"))?,
        session_id: SageAgentSessionId::parse(row.session_id.clone())
            .map_err(|_| StoreError::Corrupt("session id"))?,
        runtime_epoch: RuntimeEpoch::new(
            u64::try_from(row.runtime_epoch).map_err(|_| StoreError::Corrupt("runtime epoch"))?,
        )
        .map_err(|_| StoreError::Corrupt("runtime epoch"))?,
        ordinal: u64::try_from(row.ordinal).map_err(|_| StoreError::Corrupt("turn ordinal"))?,
        state: super::send::parse_turn_state(&row.state)?,
        revision: EntityRevision::new(
            u64::try_from(row.revision).map_err(|_| StoreError::Corrupt("turn revision"))?,
        )
        .map_err(|_| StoreError::Corrupt("turn revision"))?,
        created_at: row.created_at,
        updated_at: row.updated_at,
        terminal_at: row.terminal_at,
        items: BoundedVec::new("historyPageItems", items)
            .map_err(|_| StoreError::Corrupt("item page bound"))?,
        activities: BoundedVec::new("turnActivities", activities)
            .map_err(|_| StoreError::Corrupt("activity page bound"))?,
    })
}
