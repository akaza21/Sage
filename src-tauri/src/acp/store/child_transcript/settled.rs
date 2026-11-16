//! A subagent can settle, or start another turn, after the parent turn that
//! spawned it has ended. That turn's spawn row is never rewritten, so it keeps
//! reporting the child as the runtime last wrote it. Every path that observes
//! a child's turn start or end records the child's current state here, once,
//! at write time; the parent projection reads that small row back for an
//! ended turn and never parses the child's transcript. A recorded change that
//! an ended turn's row must show moves the parent projection, so every view of
//! the row is pushed it.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use sage_acp_protocol::codex_protocol::ChildTranscriptSnapshot;
use sage_acp_protocol::contract::{ProjectionRevision, SafeText};
use sage_acp_protocol::scheduler::RuntimeOwner;

use super::ChildTranscriptScope;
use crate::acp::projection::{AcpChatPage, ItemContent, ItemProjection};
use crate::acp::store::StoreError;
use crate::acp::store::facts::{next_projection_revision, publish_send_neutral_projection};

fn live(state: &str) -> bool {
    matches!(state, "running" | "pendingInit")
}

/// The agent state a child is in once a turn of it has this status.
pub(super) fn turn_state(status: &str) -> &'static str {
    match status {
        "completed" => "completed",
        "failed" => "errored",
        "interrupted" => "interrupted",
        _ => "running",
    }
}

/// The state a transcript's latest turn leaves its child in, if it has one.
pub(super) fn snapshot_state(snapshot: &ChildTranscriptSnapshot) -> Option<&'static str> {
    snapshot.turns.last().map(|turn| turn_state(&turn.status))
}

/// Records the child's current state. When that changes what an ended spawn
/// turn's row shows (a row reporting the child live shows it running until a
/// state is recorded), the parent projection moves and the new revision is
/// returned for publishing.
pub(super) fn record_child_state(
    transaction: &Transaction<'_>,
    scope: &ChildTranscriptScope,
    state: &'static str,
    now_millis: i64,
) -> Result<Option<ProjectionRevision>, StoreError> {
    let previous: Option<String> = transaction
        .query_row(
            "SELECT state FROM acp_child_thread_states WHERE native_child_thread_id=?1",
            [&scope.child_thread_id],
            |row| row.get(0),
        )
        .optional()?;
    if previous.as_deref() == Some(state) {
        return Ok(None);
    }
    transaction.execute(
        "INSERT INTO acp_child_thread_states
         (native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,state,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(native_child_thread_id) DO UPDATE SET
           state=excluded.state,updated_at=excluded.updated_at",
        params![
            scope.child_thread_id,
            scope.owner.project_id.as_str(),
            scope.owner.chat_id.as_str(),
            scope.owner.session_id.as_str(),
            scope.owner.runtime_epoch.get() as i64,
            state,
            now_millis,
        ],
    )?;
    if previous.as_deref().unwrap_or("running") == state {
        return Ok(None);
    }
    let shown: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM items i
           JOIN turns t ON t.id=i.turn_id AND t.project_id=i.project_id
            AND t.chat_id=i.chat_id AND t.session_id=i.session_id
           WHERE i.id=?1 AND i.kind='collaboration' AND t.terminal_at IS NOT NULL
             AND EXISTS(SELECT 1 FROM json_each(i.payload_json,'$.payload.agentStates') s
               WHERE json_extract(s.value,'$[0]')=?2
                 AND json_extract(s.value,'$[1]') IN ('running','pendingInit')))",
        params![scope.item_id, scope.child_thread_id],
        |row| row.get(0),
    )?;
    if !shown {
        return Ok(None);
    }
    let project = scope.owner.project_id.as_str();
    let chat = scope.owner.chat_id.as_str();
    let revision = next_projection_revision(transaction, project, chat)?;
    publish_send_neutral_projection(
        transaction,
        project,
        chat,
        scope.owner.session_id.as_str(),
        revision,
        now_millis,
    )?;
    Ok(Some(revision))
}

fn recorded_state(
    connection: &Connection,
    item_id: &str,
    child: &str,
) -> Result<Option<(String, i64)>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT s.state,s.updated_at FROM acp_child_thread_bindings b
             JOIN acp_child_thread_states s
               ON s.native_child_thread_id=b.native_child_thread_id
              AND s.project_id=b.project_id AND s.chat_id=b.chat_id
              AND s.session_id=b.session_id AND s.runtime_epoch=b.runtime_epoch
             WHERE b.item_id=?1 AND b.native_child_thread_id=?2",
            params![item_id, child],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
}

/// Projects each child an ended turn's row still reports live with the state
/// recorded for it since. A row that settles a child this way was last
/// updated when that end was recorded, so its times span the child's run.
pub(crate) fn project_settled_children(
    connection: &Connection,
    item: &mut ItemProjection,
) -> Result<(), StoreError> {
    let id = item.id.as_str().to_owned();
    let ItemContent::Collaboration { agent_states, .. } = &mut item.content else {
        return Ok(());
    };
    for (key, state) in agent_states.as_mut_slice() {
        if !live(state.as_str()) {
            continue;
        }
        let Some((recorded, updated_at)) = recorded_state(connection, &id, key.as_str())? else {
            continue;
        };
        if recorded != "running" {
            *state = SafeText::new("agentState", recorded)
                .map_err(|_| StoreError::Corrupt("agent state"))?;
            item.updated_at = item.updated_at.max(updated_at);
        }
    }
    Ok(())
}

/// Reports as `unknown` each child an ended turn's row still reports live once
/// nothing can still run it: its session is no longer the chat's current one,
/// or the runtime that spawned it no longer `runs`, as after an exit, a
/// replacement or an app restart. Nothing recorded how such a child ended.
pub(crate) fn settle_lost_children(
    page: &mut AcpChatPage,
    runs: impl Fn(&RuntimeOwner) -> bool,
) -> Result<(), StoreError> {
    let current = page.current_session_id.clone();
    for turn in page.turns.as_mut_slice() {
        if turn.terminal_at.is_none() {
            continue;
        }
        for item in turn.items.as_mut_slice() {
            let ItemContent::Collaboration { agent_states, .. } = &mut item.content else {
                continue;
            };
            if !agent_states
                .as_slice()
                .iter()
                .any(|(_, state)| live(state.as_str()))
            {
                continue;
            }
            let owner = RuntimeOwner {
                project_id: item.project_id.clone(),
                chat_id: item.chat_id.clone(),
                session_id: item.session_id.clone(),
                runtime_epoch: item.runtime_epoch,
            };
            if current.as_ref() == Some(&owner.session_id) && runs(&owner) {
                continue;
            }
            for (_, state) in agent_states.as_mut_slice() {
                if live(state.as_str()) {
                    *state = SafeText::new("agentState", "unknown")
                        .map_err(|_| StoreError::Corrupt("agent state"))?;
                }
            }
        }
    }
    Ok(())
}
