//! A subagent can settle, or start another turn, after the parent turn that
//! spawned it has ended. That turn's spawn row is never rewritten after, so it
//! keeps reporting the child as the runtime last wrote it. Every path that observes
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

/// The latest turn of a transcript and the state it leaves its child in.
pub(super) fn snapshot_state(snapshot: &ChildTranscriptSnapshot) -> Option<(&str, &'static str)> {
    snapshot
        .turns
        .last()
        .map(|turn| (turn.id.as_str(), turn_state(&turn.status)))
}

/// A recorded state taken with when it was recorded.
type Recorded = (String, i64);

/// What an ended turn's row shows for a child: the state recorded for it
/// when the row still reports it live or the record is newer than the row,
/// otherwise the row's own state, so a child that ran again after its row
/// was rewritten shows its new state and one the row outlived does not.
fn shown<'a>(row: &'a str, row_updated_at: i64, recorded: Option<&'a Recorded>) -> &'a str {
    match recorded {
        Some((state, at)) if live(row) || *at > row_updated_at => state,
        _ => row,
    }
}

fn same(left: &str, right: &str) -> bool {
    left == right || (live(left) && live(right))
}

/// Records the child's current state, taken from the latest turn it was seen
/// in. A turn that ended never runs again, so a late item of it is ignored.
/// When the record changes what an ended spawn turn's row shows, the parent
/// projection moves and the new revision is returned for publishing.
pub(super) fn record_child_state(
    transaction: &Transaction<'_>,
    scope: &ChildTranscriptScope,
    turn_id: &str,
    state: &'static str,
    now_millis: i64,
) -> Result<Option<ProjectionRevision>, StoreError> {
    let previous: Option<(String, Option<String>, i64)> = transaction
        .query_row(
            "SELECT state,turn_id,updated_at FROM acp_child_thread_states
             WHERE native_child_thread_id=?1",
            [&scope.child_thread_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if let Some((recorded, turn, _)) = &previous
        && (recorded == state
            || (state == "running" && turn.as_deref() == Some(turn_id) && !live(recorded)))
    {
        return Ok(None);
    }
    transaction.execute(
        "INSERT INTO acp_child_thread_states
         (native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,state,turn_id,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
         ON CONFLICT(native_child_thread_id) DO UPDATE SET
           state=excluded.state,turn_id=excluded.turn_id,updated_at=excluded.updated_at",
        params![
            scope.child_thread_id,
            scope.owner.project_id.as_str(),
            scope.owner.chat_id.as_str(),
            scope.owner.session_id.as_str(),
            scope.owner.runtime_epoch.get() as i64,
            state,
            turn_id,
            now_millis,
        ],
    )?;
    let row: Option<(Option<String>, i64)> = transaction
        .query_row(
            "SELECT (SELECT json_extract(s.value,'$[1]')
                       FROM json_each(i.payload_json,'$.payload.agentStates') s
                      WHERE json_extract(s.value,'$[0]')=?2),
                    i.updated_at
               FROM items i
               JOIN turns t ON t.id=i.turn_id AND t.project_id=i.project_id
                AND t.chat_id=i.chat_id AND t.session_id=i.session_id
              WHERE i.id=?1 AND i.kind='collaboration' AND t.terminal_at IS NOT NULL",
            params![scope.item_id, scope.child_thread_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((Some(row_state), row_updated_at)) = row else {
        return Ok(None);
    };
    let before = previous.map(|(state, _, at)| (state, at));
    let after = (state.to_owned(), now_millis);
    if same(
        shown(&row_state, row_updated_at, before.as_ref()),
        shown(&row_state, row_updated_at, Some(&after)),
    ) {
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
) -> Result<Option<Recorded>, rusqlite::Error> {
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

/// Projects each child of an ended turn's row with what that row shows for
/// it (see `shown`). A row that settles a child this way was last updated
/// when that end was recorded, so its times span the child's run.
pub(crate) fn project_settled_children(
    connection: &Connection,
    item: &mut ItemProjection,
) -> Result<(), StoreError> {
    let id = item.id.as_str().to_owned();
    let row_updated_at = item.updated_at;
    let ItemContent::Collaboration { agent_states, .. } = &mut item.content else {
        return Ok(());
    };
    for (key, state) in agent_states.as_mut_slice() {
        let recorded = recorded_state(connection, &id, key.as_str())?;
        let projected = shown(state.as_str(), row_updated_at, recorded.as_ref());
        if same(projected, state.as_str()) {
            continue;
        }
        let projected = projected.to_owned();
        if !live(&projected) {
            item.updated_at = item.updated_at.max(recorded.map_or(0, |(_, at)| at));
        }
        *state = SafeText::new("agentState", projected)
            .map_err(|_| StoreError::Corrupt("agent state"))?;
    }
    Ok(())
}

/// Reports as `unknown` each child an ended turn's row still reports live once
/// nothing can still run it: its session is no longer the chat's current one,
/// its runtime epoch has durably ended or been replaced, or no runtime in this
/// process `runs` it, as after an app restart. Nothing recorded how such a
/// child ended.
pub(crate) fn settle_lost_children(
    page: &mut AcpChatPage,
    runs: impl Fn(&RuntimeOwner) -> bool,
) -> Result<(), StoreError> {
    let current = page.current_session_id.clone();
    let live_epoch = page
        .sessions
        .as_slice()
        .iter()
        .find(|session| Some(&session.id) == current.as_ref())
        .and_then(|session| session.runtime_epoch);
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
            if current.as_ref() == Some(&owner.session_id)
                && live_epoch == Some(owner.runtime_epoch)
                && runs(&owner)
            {
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
