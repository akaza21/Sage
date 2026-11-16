//! A subagent can settle after the parent turn that spawned it has ended.
//! That turn's spawn row is never rewritten, so it keeps reporting the child
//! live; the child's stored transcript holds how it ended. The parent
//! projection is the one place that settles such a row: it takes the stored
//! end, or, once the runtime that ran the child is gone without one, reports
//! the outcome as unknown. Storing an end moves the parent projection, as a
//! runtime exit does, so every view of the row is pushed the change.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use sage_acp_protocol::contract::SafeText;

use crate::acp::projection::{ItemContent, ItemProjection};
use crate::acp::store::StoreError;
use crate::acp::store::facts::{next_projection_revision, publish_send_neutral_projection};

fn live(state: &str) -> bool {
    matches!(state, "running" | "pendingInit")
}

/// The agent state a parent row words a child's stored end as, once the last
/// turn of its transcript has settled.
fn stored_state(
    connection: &Connection,
    item_id: &str,
    child: &str,
) -> Result<Option<&'static str>, rusqlite::Error> {
    let status: Option<Option<String>> = connection
        .query_row(
            "SELECT json_extract(t.snapshot_json,'$.turns[#-1].status')
             FROM acp_child_thread_bindings b
             JOIN acp_child_thread_transcripts t
               ON t.native_child_thread_id=b.native_child_thread_id
              AND t.project_id=b.project_id AND t.chat_id=b.chat_id
              AND t.session_id=b.session_id AND t.runtime_epoch=b.runtime_epoch
             WHERE b.item_id=?1 AND b.native_child_thread_id=?2",
            params![item_id, child],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match status.flatten().as_deref() {
        Some("completed") => Some("completed"),
        Some("failed") => Some("errored"),
        Some("interrupted") => Some("interrupted"),
        _ => None,
    })
}

/// Whether the runtime that ran an item has exited or been replaced.
fn runtime_gone(connection: &Connection, item: &ItemProjection) -> Result<bool, rusqlite::Error> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_runtime_epochs
           WHERE session_id=?1 AND (runtime_epoch>?2 OR (runtime_epoch=?2 AND state='exited')))",
        params![item.session_id.as_str(), item.runtime_epoch.get() as i64],
        |row| row.get(0),
    )
}

/// Projects each child an ended turn's row still reports live with the end
/// its stored transcript settled on, or as `unknown` once its runtime is gone
/// without one: the child cannot still run, but nothing says how it ended.
pub(crate) fn project_settled_children(
    connection: &Connection,
    item: &mut ItemProjection,
) -> Result<(), StoreError> {
    let ItemContent::Collaboration { agent_states, .. } = &item.content else {
        return Ok(());
    };
    if !agent_states
        .as_slice()
        .iter()
        .any(|(_, state)| live(state.as_str()))
    {
        return Ok(());
    }
    let gone = runtime_gone(connection, item)?;
    let id = item.id.as_str().to_owned();
    let ItemContent::Collaboration { agent_states, .. } = &mut item.content else {
        return Ok(());
    };
    for (key, state) in agent_states.as_mut_slice() {
        if !live(state.as_str()) {
            continue;
        }
        let settled = match stored_state(connection, &id, key.as_str())? {
            Some(settled) => settled,
            None if gone => "unknown",
            None => continue,
        };
        *state =
            SafeText::new("agentState", settled).map_err(|_| StoreError::Corrupt("agent state"))?;
    }
    Ok(())
}

/// After a child's end is stored, moves the parent projection when an ended
/// turn's spawn row still reports that child live, so the row is re-projected.
pub(super) fn publish_settled_child(
    transaction: &Transaction<'_>,
    item_id: &str,
    child: &str,
    now_millis: i64,
) -> Result<(), StoreError> {
    let row: Option<(String, String, String)> = transaction
        .query_row(
            "SELECT i.project_id,i.chat_id,i.session_id FROM items i
             JOIN turns t ON t.id=i.turn_id AND t.project_id=i.project_id
              AND t.chat_id=i.chat_id AND t.session_id=i.session_id
             WHERE i.id=?1 AND i.kind='collaboration' AND t.terminal_at IS NOT NULL
               AND EXISTS(SELECT 1 FROM json_each(i.payload_json,'$.payload.agentStates') s
                 WHERE json_extract(s.value,'$[0]')=?2
                   AND json_extract(s.value,'$[1]') IN ('running','pendingInit'))",
            params![item_id, child],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((project, chat, session)) = row else {
        return Ok(());
    };
    if stored_state(transaction, item_id, child)?.is_none() {
        return Ok(());
    }
    let revision = next_projection_revision(transaction, &project, &chat)?;
    publish_send_neutral_projection(transaction, &project, &chat, &session, revision, now_millis)
}
