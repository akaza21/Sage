use sage_acp_protocol::codex_protocol::{ChildTranscriptSnapshot, ChildTurn};

use super::ChildTranscriptStoreError;
use super::live::{validate_bounds, validate_turn_ids};

/// Merges a native read into the retained transcript. Turns stay in the
/// read's order, which is the child's own; a turn only the retained
/// transcript holds keeps its place next to the retained turns around it, or
/// goes last as the newest. A read naming a turn twice is rejected.
pub(super) fn merge_snapshots(
    read: &ChildTranscriptSnapshot,
    retained: &ChildTranscriptSnapshot,
) -> Result<ChildTranscriptSnapshot, ChildTranscriptStoreError> {
    if read.child_thread_id != retained.child_thread_id
        || read.parent_thread_id != retained.parent_thread_id
    {
        return Err(ChildTranscriptStoreError::Stale);
    }
    validate_turn_ids(read)?;
    let mut turns: Vec<ChildTurn> = read
        .turns
        .iter()
        .map(
            |read_turn| match retained.turns.iter().find(|turn| turn.id == read_turn.id) {
                Some(turn) => {
                    let mut turn = turn.clone();
                    merge_turn(&mut turn, read_turn);
                    turn
                }
                None => read_turn.clone(),
            },
        )
        .collect();
    let at = |turns: &[ChildTurn], id: &str| turns.iter().position(|turn| turn.id == id);
    for (index, turn) in retained.turns.iter().enumerate() {
        if at(&turns, &turn.id).is_some() {
            continue;
        }
        let position = retained.turns[..index]
            .iter()
            .rev()
            .find_map(|earlier| at(&turns, &earlier.id).map(|position| position + 1))
            .or_else(|| {
                retained.turns[index + 1..]
                    .iter()
                    .find_map(|later| at(&turns, &later.id))
            })
            .unwrap_or(turns.len());
        turns.insert(position, turn.clone());
    }
    let merged = ChildTranscriptSnapshot {
        turns,
        retention_limited: retained.retention_limited || read.retention_limited,
        ..retained.clone()
    };
    validate_bounds(&merged)?;
    Ok(merged)
}

fn merge_turn(turn: &mut ChildTurn, read_turn: &ChildTurn) {
    if turn.status == "inProgress" {
        turn.status = read_turn.status.clone();
    }
    for (read_index, item) in read_turn.items.iter().enumerate() {
        if let Some(candidate) = turn
            .items
            .iter_mut()
            .find(|candidate| candidate.id == item.id)
        {
            if !candidate.lifecycle.is_terminal()
                || (item.lifecycle.is_terminal() && candidate.observed_sequence.is_none())
            {
                let sequence = candidate.observed_sequence;
                let role = candidate.role;
                *candidate = item.clone();
                candidate.observed_sequence = sequence;
                if candidate.role.is_none() {
                    candidate.role = role;
                }
            }
        } else {
            let before = read_turn.items[read_index + 1..].iter().find_map(|next| {
                turn.items
                    .iter()
                    .position(|candidate| candidate.id == next.id)
            });
            let position = before.unwrap_or(turn.items.len());
            turn.items.insert(position, item.clone());
        }
    }
}
