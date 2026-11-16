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

/// A transcript's latest turn as an observation of the state it leaves its
/// child in, if it has one. A read learns no end time.
pub(super) fn snapshot_state(snapshot: &ChildTranscriptSnapshot) -> Option<Observed<'_>> {
    snapshot.turns.last().map(|turn| Observed {
        turn: &turn.id,
        state: turn_state(&turn.status),
        ended_at: None,
    })
}

/// How many of a child's latest ended turns its record remembers by id, so a
/// late item of one its transcript no longer holds never reads as the child
/// running again.
const ENDED_TURNS: usize = 32;

/// A child turn seen starting or ending; `ended_at` is set only for an end
/// observed live.
pub(super) struct Observed<'a> {
    pub(super) turn: &'a str,
    pub(super) state: &'static str,
    pub(super) ended_at: Option<i64>,
}

/// A child's recorded state and its provenance: the turn it came from, that
/// turn's place in the child's order, and whether the state is known to be
/// newer than the spawn row (recorded after the spawn turn ended, or on
/// upgrade, from a later turn than the one the child was spawned with).
struct Recorded {
    turn: String,
    ordinal: i64,
    state: String,
    spawn_ended: bool,
    ended_at: Option<i64>,
}

/// What an ended turn's row shows for a child: the state recorded for it
/// when the row still reports it live or the record is newer than the row,
/// otherwise the row's own state. So a child that ran again after its row was
/// rewritten shows its new state, and one the row outlived does not, whatever
/// any clock says.
fn applies(row: &str, recorded: &Recorded) -> bool {
    live(row) || recorded.spawn_ended
}

fn shown<'a>(row: &'a str, recorded: Option<&'a Recorded>) -> &'a str {
    match recorded {
        Some(recorded) if applies(row, recorded) => &recorded.state,
        _ => row,
    }
}

fn same(left: &str, right: &str) -> bool {
    left == right || (live(left) && live(right))
}

/// The place in the child's order that `turn` takes, or `None` when it is
/// older than the recorded turn and so cannot change the record. The
/// transcript orders the turns it holds, and the record is never behind its
/// last one; a turn the transcript does not hold is the recorded one, one the
/// record saw end, or a new turn after both.
fn ordinal(
    transcript: &ChildTranscriptSnapshot,
    turn: &str,
    recorded: Option<&Recorded>,
    ended_turns: &[String],
) -> Option<i64> {
    let held = transcript.turns.len();
    match transcript
        .turns
        .iter()
        .position(|candidate| candidate.id == turn)
    {
        Some(index) if index + 1 < held => None,
        Some(index) => Some(index as i64),
        None => match recorded {
            Some(recorded) if recorded.turn == turn => Some(recorded.ordinal),
            Some(_) if ended_turns.iter().any(|ended| ended == turn) => None,
            Some(recorded) => Some((held as i64).max(recorded.ordinal + 1)),
            None => Some(held as i64),
        },
    }
}

/// Records the state `observed` leaves the child in, given the child's
/// stored `transcript` before the observation is applied to it. The record
/// owns the child's current turn: a turn older than it changes nothing, and
/// a turn ends once. When the record changes what an ended spawn turn's row
/// shows, the parent projection moves and the new revision is returned for
/// publishing.
pub(super) fn record_child_state(
    transaction: &Transaction<'_>,
    scope: &ChildTranscriptScope,
    transcript: &ChildTranscriptSnapshot,
    observed: Observed<'_>,
    now_millis: i64,
) -> Result<Option<ProjectionRevision>, StoreError> {
    let previous: Option<(Recorded, String)> = transaction
        .query_row(
            "SELECT turn_id,turn_ordinal,state,spawn_ended,ended_at,ended_turns
               FROM acp_child_thread_states WHERE native_child_thread_id=?1",
            [&scope.child_thread_id],
            |row| {
                Ok((
                    Recorded {
                        turn: row.get(0)?,
                        ordinal: row.get(1)?,
                        state: row.get(2)?,
                        spawn_ended: row.get(3)?,
                        ended_at: row.get(4)?,
                    },
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    let (previous, mut ended_turns) = match previous {
        Some((recorded, ended)) => (
            Some(recorded),
            serde_json::from_str::<Vec<String>>(&ended)
                .map_err(|_| StoreError::Corrupt("child thread state"))?,
        ),
        None => (None, Vec::new()),
    };
    let Some(ordinal) = ordinal(transcript, observed.turn, previous.as_ref(), &ended_turns) else {
        return Ok(None);
    };
    let ordinal = match &previous {
        Some(recorded) if recorded.turn == observed.turn => {
            if !live(&recorded.state) || live(observed.state) {
                return Ok(None);
            }
            recorded.ordinal
        }
        Some(recorded) if ordinal <= recorded.ordinal => return Ok(None),
        _ => ordinal,
    };
    if !live(observed.state) {
        if ended_turns.len() == ENDED_TURNS {
            ended_turns.remove(0);
        }
        ended_turns.push(observed.turn.to_owned());
    }
    let row: Option<(Option<String>, bool)> = transaction
        .query_row(
            "SELECT (SELECT json_extract(s.value,'$[1]')
                       FROM json_each(i.payload_json,'$.payload.agentStates') s
                      WHERE json_extract(s.value,'$[0]')=?2),
                    t.terminal_at IS NOT NULL
               FROM items i
               JOIN turns t ON t.id=i.turn_id AND t.project_id=i.project_id
                AND t.chat_id=i.chat_id AND t.session_id=i.session_id
              WHERE i.id=?1 AND i.kind='collaboration'",
            params![scope.item_id, scope.child_thread_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let after = Recorded {
        turn: observed.turn.to_owned(),
        ordinal,
        state: observed.state.to_owned(),
        spawn_ended: row.as_ref().is_some_and(|(_, ended)| *ended),
        ended_at: observed.ended_at.filter(|_| !live(observed.state)),
    };
    transaction.execute(
        "INSERT INTO acp_child_thread_states
         (native_child_thread_id,project_id,chat_id,session_id,runtime_epoch,turn_id,
          turn_ordinal,state,spawn_ended,ended_at,ended_turns)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
         ON CONFLICT(native_child_thread_id) DO UPDATE SET
           turn_id=excluded.turn_id,turn_ordinal=excluded.turn_ordinal,state=excluded.state,
           spawn_ended=excluded.spawn_ended,ended_at=excluded.ended_at,
           ended_turns=excluded.ended_turns",
        params![
            scope.child_thread_id,
            scope.owner.project_id.as_str(),
            scope.owner.chat_id.as_str(),
            scope.owner.session_id.as_str(),
            scope.owner.runtime_epoch.get() as i64,
            after.turn,
            after.ordinal,
            after.state,
            after.spawn_ended,
            after.ended_at,
            serde_json::to_string(&ended_turns).expect("turn ids serialize"),
        ],
    )?;
    let Some((Some(row_state), true)) = row else {
        return Ok(None);
    };
    if same(
        shown(&row_state, previous.as_ref()),
        shown(&row_state, Some(&after)),
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
            "SELECT s.turn_id,s.turn_ordinal,s.state,s.spawn_ended,s.ended_at
               FROM acp_child_thread_bindings b
               JOIN acp_child_thread_states s
                 ON s.native_child_thread_id=b.native_child_thread_id
                AND s.project_id=b.project_id AND s.chat_id=b.chat_id
                AND s.session_id=b.session_id AND s.runtime_epoch=b.runtime_epoch
              WHERE b.item_id=?1 AND b.native_child_thread_id=?2",
            params![item_id, child],
            |row| {
                Ok(Recorded {
                    turn: row.get(0)?,
                    ordinal: row.get(1)?,
                    state: row.get(2)?,
                    spawn_ended: row.get(3)?,
                    ended_at: row.get(4)?,
                })
            },
        )
        .optional()
}

/// Projects each child of an ended turn's row with what that row shows for
/// it (see `applies`). The row's times then end when the latest child it
/// settled was seen to end; a child whose end time is not known is listed so
/// that no time is claimed for it.
pub(crate) fn project_settled_children(
    connection: &Connection,
    item: &mut ItemProjection,
) -> Result<(), StoreError> {
    let id = item.id.as_str().to_owned();
    let mut ended_at = None;
    let ItemContent::Collaboration {
        agent_states,
        untimed_agents,
        ..
    } = &mut item.content
    else {
        return Ok(());
    };
    for (key, state) in agent_states.as_mut_slice() {
        let Some(recorded) = recorded_state(connection, &id, key.as_str())? else {
            continue;
        };
        if !applies(state.as_str(), &recorded) {
            continue;
        }
        if !live(&recorded.state) {
            match recorded.ended_at {
                Some(at) => ended_at = ended_at.max(Some(at)),
                None => untimed_agents.push(key.clone()),
            }
        }
        if !same(&recorded.state, state.as_str()) {
            *state = SafeText::new("agentState", recorded.state)
                .map_err(|_| StoreError::Corrupt("agent state"))?;
        }
    }
    if let Some(at) = ended_at {
        item.updated_at = at;
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
