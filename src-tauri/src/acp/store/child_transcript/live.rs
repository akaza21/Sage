use rusqlite::{OptionalExtension, Transaction, params};

use crate::acp::{
    bindings::codex::{
        ChildItem, ChildTranscriptSnapshot, ChildTurn, MAX_ITEMS, MAX_SNAPSHOT_BYTES, MAX_TURNS,
    },
    store::{AdapterStoreOwner, ChildLiveItemCommand, StoreError},
};

use super::{
    ChildTranscriptScope, ChildTranscriptStoreError, load_snapshot, verify_current_binding,
    write_snapshot_in,
};

pub(crate) fn persist_live_item_in(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    command: &ChildLiveItemCommand,
) -> Result<bool, StoreError> {
    let item_id: Option<String> = transaction
        .query_row(
            "SELECT b.item_id FROM acp_child_thread_bindings b
             JOIN acp_child_thread_owners o ON o.native_child_thread_id=b.native_child_thread_id
             WHERE b.native_child_thread_id=?1 AND b.project_id=?2 AND b.chat_id=?3
               AND b.session_id=?4 AND b.runtime_epoch=?5 AND b.turn_id=?6
               AND o.parent_thread_id=?7 AND o.source='codexSubAgentActivity'",
            params![
                command.child_thread_id.as_str(),
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64,
                owner.turn_id.as_str(),
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
        owner: crate::acp::scheduler::RuntimeOwner {
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
    let mut candidate = snapshot.clone();
    match apply_live_item(&mut candidate, command) {
        Ok(true) => {
            write_snapshot_in(transaction, &scope, &candidate, command.now_millis)
                .map_err(store_error)?;
            Ok(true)
        }
        Ok(false) => Ok(false),
        Err(StoreError::Capacity { .. }) if !snapshot.retention_limited => {
            snapshot.retention_limited = true;
            write_snapshot_in(transaction, &scope, &snapshot, command.now_millis)
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
        lifecycle: command.lifecycle,
        tool_state: command.tool_state,
        payload: command.payload.clone(),
    };
    let changed = if let Some(item) = turn.items.iter_mut().find(|item| item.id == next.id) {
        if (item.lifecycle.is_terminal() && !next.lifecycle.is_terminal()) || *item == next {
            false
        } else {
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

pub(super) fn merge_snapshots(
    read: &ChildTranscriptSnapshot,
    retained: &ChildTranscriptSnapshot,
) -> Result<ChildTranscriptSnapshot, ChildTranscriptStoreError> {
    if read.child_thread_id != retained.child_thread_id
        || read.parent_thread_id != retained.parent_thread_id
    {
        return Err(ChildTranscriptStoreError::Stale);
    }
    let mut merged = read.clone();
    merged.retention_limited |= retained.retention_limited;
    for retained_turn in &retained.turns {
        if let Some(turn) = merged
            .turns
            .iter_mut()
            .find(|turn| turn.id == retained_turn.id)
        {
            for item in &retained_turn.items {
                match turn
                    .items
                    .iter_mut()
                    .find(|candidate| candidate.id == item.id)
                {
                    Some(candidate)
                        if item.lifecycle.is_terminal() && !candidate.lifecycle.is_terminal() =>
                    {
                        *candidate = item.clone();
                    }
                    Some(_) => {}
                    None => turn.items.push(item.clone()),
                }
            }
        } else {
            merged.turns.push(retained_turn.clone());
        }
    }
    validate_bounds(&merged)?;
    Ok(merged)
}

pub(super) fn validate_bounds(
    snapshot: &ChildTranscriptSnapshot,
) -> Result<(), ChildTranscriptStoreError> {
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
    }
}
