use std::collections::HashSet;

use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::store::{AdapterStoreOwner, ChildLiveItemCommand, StoreError};
use sage_acp_protocol::{
    codex_protocol::{
        ChildItem, ChildTranscriptSnapshot, ChildTurn, MAX_ITEMS, MAX_SNAPSHOT_BYTES, MAX_TURNS,
    },
    intent::ExternalId,
    lifecycle::TurnState,
};

use super::settled::{Observed, record_child_state, turn_state};
use super::{
    ChildTranscriptScope, ChildTranscriptStoreError, load_snapshot, verify_current_binding,
    write_snapshot_in,
};

/// A changed child snapshot advances the chat's child projection epoch (its
/// newest live `updated_at`, see `child_projection_epoch`). It moves the
/// parent projection revision, the optimistic-concurrency token of user
/// actions, only when the child's recorded state changes what an ended spawn
/// turn's row shows (see `settled`). Duplicates stay silent.
pub(crate) fn persist_live_item_in(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    command: &ChildLiveItemCommand,
) -> Result<bool, StoreError> {
    store_live_item_in(transaction, owner, command)
}

pub(crate) fn persist_turn_status_in(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    child_thread_id: &ExternalId,
    child_turn_id: &ExternalId,
    status: TurnState,
    now_millis: i64,
) -> Result<bool, StoreError> {
    store_turn_status_in(
        transaction,
        owner,
        child_thread_id,
        child_turn_id,
        status,
        now_millis,
    )
}

fn store_live_item_in(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    command: &ChildLiveItemCommand,
) -> Result<bool, StoreError> {
    let item_id: Option<String> = transaction
        .query_row(
            "SELECT b.item_id FROM acp_child_thread_bindings b
             JOIN acp_child_thread_owners o ON o.native_child_thread_id=b.native_child_thread_id
             WHERE b.native_child_thread_id=?1 AND b.project_id=?2 AND b.chat_id=?3
               AND b.session_id=?4 AND b.runtime_epoch=?5
               AND o.parent_thread_id=?6 AND o.source='codexSubAgentActivity'",
            params![
                command.child_thread_id.as_str(),
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64,
                owner.external_session_id.as_str(),
            ],
            |row| row.get(0),
        )
        .optional()?;
    let Some(item_id) = item_id else {
        return Err(StoreError::Conflict {
            resource: "childTranscriptOwner",
        });
    };
    let scope = ChildTranscriptScope {
        owner: sage_acp_protocol::scheduler::RuntimeOwner {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
        },
        item_id,
        child_thread_id: command.child_thread_id.as_str().to_owned(),
        parent_thread_id: owner.external_session_id.as_str().to_owned(),
    };
    verify_current_binding(transaction, &scope).map_err(store_error)?;
    let mut snapshot = load_snapshot(transaction, &scope)
        .map_err(store_error)?
        .unwrap_or_else(|| ChildTranscriptSnapshot {
            child_thread_id: scope.child_thread_id.clone(),
            parent_thread_id: scope.parent_thread_id.clone(),
            turns: Vec::new(),
            retention_limited: false,
        });
    // A turn the transcript holds was recorded when it first arrived, so its
    // later deltas leave the record alone.
    let turn = command.child_turn_id.as_str();
    if !snapshot.turns.iter().any(|held| held.id == turn) {
        record_child_state(
            transaction,
            &scope,
            &snapshot,
            Observed {
                turn,
                state: "running",
                ended_at: None,
            },
            command.now_millis,
        )?;
    }
    let mut candidate = snapshot.clone();
    if snapshot.retention_limited
        && !snapshot.turns.iter().any(|turn| {
            turn.items
                .iter()
                .any(|item| item.id == command.item_id.as_str())
        })
    {
        return Ok(false);
    }
    match apply_live_item(&mut candidate, command) {
        Ok(true) => {
            write_snapshot_in(transaction, &scope, &candidate, command.now_millis, true)
                .map_err(store_error)?;
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(StoreError::Capacity { .. }) if !snapshot.retention_limited => {
            snapshot.retention_limited = true;
            write_snapshot_in(transaction, &scope, &snapshot, command.now_millis, true)
                .map_err(store_error)?;
            Ok(true)
        }
        Err(StoreError::Capacity { .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

fn apply_live_item(
    snapshot: &mut ChildTranscriptSnapshot,
    command: &ChildLiveItemCommand,
) -> Result<bool, StoreError> {
    let turn_id = command.child_turn_id.as_str();
    let total_items = snapshot
        .turns
        .iter()
        .map(|turn| turn.items.len())
        .sum::<usize>();
    let next_sequence = snapshot
        .turns
        .iter()
        .flat_map(|turn| &turn.items)
        .filter_map(|item| item.observed_sequence)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let turn = if let Some(turn) = snapshot.turns.iter_mut().find(|turn| turn.id == turn_id) {
        turn
    } else {
        if snapshot.turns.len() == MAX_TURNS {
            return Err(capacity());
        }
        snapshot.turns.push(ChildTurn {
            id: turn_id.to_owned(),
            status: "inProgress".into(),
            items: Vec::new(),
        });
        snapshot.turns.last_mut().expect("inserted child turn")
    };
    let next = ChildItem {
        id: command.item_id.as_str().to_owned(),
        role: command.role,
        observed_sequence: turn
            .items
            .iter()
            .find(|item| item.id == command.item_id.as_str())
            .and_then(|item| item.observed_sequence)
            .or(Some(next_sequence)),
        lifecycle: command.lifecycle,
        tool_state: command.tool_state,
        payload: command.payload.clone(),
    };
    let changed = if let Some(item) = turn.items.iter_mut().find(|item| item.id == next.id) {
        if (item.lifecycle.is_terminal() && !next.lifecycle.is_terminal()) || *item == next {
            false
        } else {
            let mut next = next;
            if next.role.is_none() {
                next.role = item.role;
            }
            *item = next;
            true
        }
    } else {
        if total_items == MAX_ITEMS {
            return Err(capacity());
        }
        turn.items.push(next);
        true
    };
    validate_bounds(snapshot).map_err(store_error)?;
    Ok(changed)
}

fn store_turn_status_in(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    child_thread_id: &ExternalId,
    child_turn_id: &ExternalId,
    status: TurnState,
    now_millis: i64,
) -> Result<bool, StoreError> {
    let status = match status {
        TurnState::Completed => "completed",
        TurnState::Interrupted => "interrupted",
        TurnState::Failed => "failed",
        _ => {
            return Err(StoreError::Conflict {
                resource: "childTurnStatus",
            });
        }
    };
    let item_id: String = transaction
        .query_row(
            "SELECT b.item_id FROM acp_child_thread_bindings b
         JOIN acp_child_thread_owners o ON o.native_child_thread_id=b.native_child_thread_id
         WHERE b.native_child_thread_id=?1 AND b.project_id=?2 AND b.chat_id=?3
           AND b.session_id=?4 AND b.runtime_epoch=?5
           AND o.parent_thread_id=?6 AND o.source='codexSubAgentActivity'",
            params![
                child_thread_id.as_str(),
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64,
                owner.external_session_id.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict {
            resource: "childTranscriptOwner",
        })?;
    let scope = ChildTranscriptScope {
        owner: sage_acp_protocol::scheduler::RuntimeOwner {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
        },
        item_id,
        child_thread_id: child_thread_id.as_str().to_owned(),
        parent_thread_id: owner.external_session_id.as_str().to_owned(),
    };
    verify_current_binding(transaction, &scope).map_err(store_error)?;
    let mut snapshot = load_snapshot(transaction, &scope)
        .map_err(store_error)?
        .unwrap_or_else(|| ChildTranscriptSnapshot {
            child_thread_id: scope.child_thread_id.clone(),
            parent_thread_id: scope.parent_thread_id.clone(),
            turns: Vec::new(),
            retention_limited: false,
        });
    record_child_state(
        transaction,
        &scope,
        &snapshot,
        Observed {
            turn: child_turn_id.as_str(),
            state: turn_state(status),
            ended_at: Some(now_millis),
        },
        now_millis,
    )?;
    let retained = snapshot.clone();
    let position = snapshot
        .turns
        .iter()
        .position(|turn| turn.id == child_turn_id.as_str());
    if let Some(index) = position {
        let turn = &mut snapshot.turns[index];
        if turn.status == status {
            return Ok(false);
        }
        if turn.status != "inProgress" {
            return Err(StoreError::Conflict {
                resource: "childTurnStatus",
            });
        }
        turn.status = status.into();
    } else {
        if snapshot.retention_limited {
            return Ok(false);
        }
        if snapshot.turns.len() == MAX_TURNS {
            snapshot.retention_limited = true;
            write_snapshot_in(transaction, &scope, &snapshot, now_millis, true)
                .map_err(store_error)?;
            return Ok(true);
        }
        snapshot.turns.push(ChildTurn {
            id: child_turn_id.as_str().to_owned(),
            status: status.into(),
            items: Vec::new(),
        });
    }
    match write_snapshot_in(transaction, &scope, &snapshot, now_millis, true) {
        Ok(()) => Ok(true),
        Err(ChildTranscriptStoreError::Capacity) if !retained.retention_limited => {
            let mut limited = retained;
            limited.retention_limited = true;
            write_snapshot_in(transaction, &scope, &limited, now_millis, true)
                .map_err(store_error)?;
            Ok(true)
        }
        Err(ChildTranscriptStoreError::Capacity) => Ok(false),
        Err(error) => Err(store_error(error)),
    }
}

/// A transcript names each turn once; one naming a turn twice has no order.
pub(super) fn validate_turn_ids(
    snapshot: &ChildTranscriptSnapshot,
) -> Result<(), ChildTranscriptStoreError> {
    let mut ids = HashSet::with_capacity(snapshot.turns.len());
    if snapshot
        .turns
        .iter()
        .all(|turn| ids.insert(turn.id.as_str()))
    {
        Ok(())
    } else {
        Err(ChildTranscriptStoreError::Corrupt)
    }
}

pub(super) fn validate_bounds(
    snapshot: &ChildTranscriptSnapshot,
) -> Result<(), ChildTranscriptStoreError> {
    validate_turn_ids(snapshot)?;
    if snapshot.turns.len() > MAX_TURNS
        || snapshot
            .turns
            .iter()
            .map(|turn| turn.items.len())
            .sum::<usize>()
            > MAX_ITEMS
        || serde_json::to_vec(snapshot)
            .map_err(|_| ChildTranscriptStoreError::Corrupt)?
            .len()
            > MAX_SNAPSHOT_BYTES - 64
    {
        return Err(ChildTranscriptStoreError::Capacity);
    }
    Ok(())
}

fn capacity() -> StoreError {
    StoreError::Capacity {
        resource: "childTranscript",
        limit: MAX_SNAPSHOT_BYTES as u64,
    }
}

fn store_error(error: ChildTranscriptStoreError) -> StoreError {
    match error {
        ChildTranscriptStoreError::Stale => StoreError::Conflict {
            resource: "childTranscriptOwner",
        },
        ChildTranscriptStoreError::Storage(error) => StoreError::Database(error),
        ChildTranscriptStoreError::Corrupt => StoreError::Corrupt("childTranscript"),
        ChildTranscriptStoreError::Capacity => capacity(),
        ChildTranscriptStoreError::Projection(error) => error,
    }
}
