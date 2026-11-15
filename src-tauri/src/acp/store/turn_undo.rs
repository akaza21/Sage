//! Undone Turns. Turns and their items are immutable, so Undo records a
//! `turnUndone` fact on the Turn instead: every reader of the visible
//! conversation leaves such a Turn out through [`visible_turn!`], and a
//! harness that cannot drop the Turn from its own thread is rewound on its
//! next launch (see [`pending_thread_rewind`]).

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use sage_acp_protocol::contract::{
    ProjectionRevision, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
};

use super::{
    StoreError,
    facts::{append_fact_payload, next_projection_revision, publish_send_neutral_projection},
};

/// SQL predicate: the Turn whose session and id are the given columns has
/// not been undone. The one definition of a visible Turn.
macro_rules! visible_turn {
    ($session:literal, $turn:literal) => {
        concat!(
            "NOT EXISTS(SELECT 1 FROM session_facts undone WHERE undone.session_id=",
            $session,
            " AND undone.turn_id=",
            $turn,
            " AND undone.kind='turnUndone')"
        )
    };
}
pub(crate) use visible_turn;

pub(crate) mod intent;
pub(crate) mod recovery;

const TURN_UNDONE: &str = "turnUndone";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UndoneThread {
    /// The harness already dropped the Turn from its own thread.
    Reverted,
    /// The next launch resumes the thread at the reply before the Turn.
    Cut,
    /// The thread holds nothing before the Turn: the next send starts a
    /// fresh native session.
    Restart,
}

impl UndoneThread {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reverted => "reverted",
            Self::Cut => "cut",
            Self::Restart => "restart",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "reverted" => Ok(Self::Reverted),
            "cut" => Ok(Self::Cut),
            "restart" => Ok(Self::Restart),
            _ => Err(StoreError::Corrupt("turn undone thread")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnUndoneCommand {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    /// The runtime that undid the Turn; until a later one runs a Turn, a
    /// rewind it leaves pending is not yet applied.
    pub runtime_epoch: RuntimeEpoch,
    pub turn_id: SageTurnId,
    /// The harness dropped the Turn from its thread itself.
    pub harness_reverted: bool,
    /// Validated Claude reply UUID; absent only for a proven empty thread.
    pub cut_at: Option<String>,
    pub now_millis: i64,
}

/// An undo as recorded: what the native thread still needs, and the page
/// revision without the Turn, unless an earlier record already published it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RecordedUndo {
    pub thread: UndoneThread,
    pub published: Option<ProjectionRevision>,
}

/// Records that the harness undid `turn_id`, and publishes the page without
/// it. A Turn is undone once: repeating the record returns the first one's
/// thread mode and changes nothing. A Turn that is no longer the Chat's
/// latest visible one (a prompt raced the undo) is refused: the harness
/// rewound past a Turn the page still shows.
pub(crate) fn record_turn_undone(
    connection: &mut Connection,
    command: &TurnUndoneCommand,
) -> Result<RecordedUndo, StoreError> {
    let transaction = connection.transaction()?;
    let recorded = record_turn_undone_in(&transaction, command)?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(recorded)
}

pub(super) fn undo_payload(command: &TurnUndoneCommand, thread: UndoneThread) -> String {
    serde_json::json!({"kind":TURN_UNDONE,"entityId":command.turn_id.as_str(),
        "state":"committed","thread":thread.as_str(),
        "runtimeEpoch":command.runtime_epoch.get(),"cutAt":command.cut_at})
    .to_string()
}

pub(super) fn record_turn_undone_in(
    transaction: &Transaction<'_>,
    command: &TurnUndoneCommand,
) -> Result<RecordedUndo, StoreError> {
    let (project, chat, session, turn) = (
        command.project_id.as_str(),
        command.chat_id.as_str(),
        command.session_id.as_str(),
        command.turn_id.as_str(),
    );
    let recorded: Option<String> = transaction
        .query_row(
            "SELECT json_extract(payload_json,'$.thread') FROM session_facts
             WHERE session_id=?1 AND turn_id=?2 AND kind=?3 LIMIT 1",
            params![session, turn, TURN_UNDONE],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(recorded) = recorded {
        return Ok(RecordedUndo {
            thread: UndoneThread::parse(&recorded)?,
            published: None,
        });
    }
    if !super::session_surface::is_undoable_turn(transaction, project, chat, session, turn)? {
        return Err(StoreError::Conflict {
            resource: "undoneTurn",
        });
    }
    let mut pending = intent::pending(transaction, project, chat)?;
    if let Some(pending) = &mut pending {
        pending.command.now_millis = command.now_millis;
        if pending.command != *command {
            return Err(StoreError::Conflict {
                resource: "undoIntent",
            });
        }
        intent::release_reservation(transaction, command)?;
    }
    let thread = if command.harness_reverted {
        UndoneThread::Reverted
    } else if command.cut_at.is_some() {
        UndoneThread::Cut
    } else {
        UndoneThread::Restart
    };
    let payload = undo_payload(command, thread);
    let projection = next_projection_revision(transaction, project, chat)?;
    append_fact_payload(
        transaction,
        project,
        chat,
        session,
        Some(turn),
        TURN_UNDONE,
        &payload,
        projection,
        command.now_millis,
    )?;
    publish_send_neutral_projection(
        transaction,
        project,
        chat,
        session,
        projection,
        command.now_millis,
    )?;
    if let Some(pending) = pending {
        intent::append_settled(
            transaction,
            &pending,
            "committed",
            projection,
            command.now_millis,
        )?;
    }
    Ok(RecordedUndo {
        thread,
        published: Some(projection),
    })
}

/// Whether Undo is withheld from `turn`, the session's latest visible
/// finished Turn: a fork Chat was taken through it and shows the parent's
/// history up to it, or, for a harness that reverts its own thread
/// (`single_step`), a Turn its runtime ran later was undone already: such a
/// harness only reverts the last Turn its process ran.
pub(crate) fn undo_withheld(
    connection: &Connection,
    project: &str,
    chat: &str,
    session: &str,
    turn: &str,
    single_step: bool,
) -> Result<bool, StoreError> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chat_lineage l
               JOIN chats c ON c.project_id=l.child_project_id AND c.id=l.child_chat_id
               WHERE l.parent_project_id=?1 AND l.parent_chat_id=?2 AND l.source_turn_id=?4)
             OR (?5 AND EXISTS(SELECT 1 FROM turns u JOIN turns later
               ON later.session_id=u.session_id AND later.runtime_epoch=u.runtime_epoch
                 AND later.ordinal>u.ordinal
               WHERE u.session_id=?3 AND u.id=?4))",
            params![project, chat, session, turn, single_step],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

/// The rewind a session's native thread still needs before it takes a
/// prompt: the latest Turn undone without the harness dropping it, until a
/// later runtime than the one that undid it has run a Turn. Every launch
/// applies a pending rewind, so a Turn of a later runtime proves it applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadRewind {
    pub turn_id: SageTurnId,
    pub cut_at: Option<String>,
    /// `Cut` or `Restart`; never `Reverted`.
    pub thread: UndoneThread,
    /// The runtime that undid the Turn, which still holds it.
    pub runtime_epoch: u64,
}

impl ThreadRewind {
    pub(crate) fn held_by(&self, epoch: RuntimeEpoch) -> bool {
        epoch.get() <= self.runtime_epoch
    }
}

/// A visible, natively correlated Turn in a later runtime proves it applied
/// the rewind. Queued and zero-byte prompts do not clear it. Reads use the
/// Turn indexes and the fact index scoped to each Turn.
pub(crate) fn pending_thread_rewind(
    connection: &Connection,
    session: &str,
) -> Result<Option<ThreadRewind>, StoreError> {
    let row: Option<(String, String, Option<i64>, Option<String>)> = connection
        .query_row(
            concat!(
                "SELECT f.turn_id,json_extract(f.payload_json,'$.thread'),
                        json_extract(f.payload_json,'$.runtimeEpoch'), json_extract(f.payload_json,'$.cutAt')
                 FROM turns u JOIN session_facts f
                   ON f.session_id=u.session_id AND f.turn_id=u.id AND f.kind='turnUndone'
                 WHERE u.session_id=?1 AND u.ordinal>COALESCE((SELECT t.ordinal FROM turns t
                     WHERE t.session_id=?1 AND t.delivery_state='correlated' AND ",
                visible_turn!("t.session_id", "t.id"),
                " ORDER BY t.ordinal DESC LIMIT 1),-1)
                 ORDER BY f.sequence DESC LIMIT 1"
            ),
            [session],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((turn, thread, epoch, cut_at)) = row else {
        return Ok(None);
    };
    let thread = UndoneThread::parse(&thread)?;
    // Without the runtime that undid the Turn, no runtime can be shown not
    // to hold it: the read fails, and with it every prepare in the session.
    let epoch = epoch.ok_or(StoreError::Corrupt("undo epoch"))?;
    let runtime_epoch = u64::try_from(epoch).map_err(|_| StoreError::Corrupt("undo epoch"))?;
    let latest_epoch: i64 = connection.query_row(
        "SELECT COALESCE(MAX(runtime_epoch),0) FROM turns WHERE session_id=?1 AND delivery_state='correlated'",
        [session],
        |row| row.get(0),
    )?;
    if thread == UndoneThread::Reverted || latest_epoch > epoch {
        return Ok(None);
    }
    Ok(Some(ThreadRewind {
        turn_id: SageTurnId::parse(turn).map_err(|_| StoreError::Corrupt("undone turn id"))?,
        thread,
        cut_at,
        runtime_epoch,
    }))
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod intent_tests;
#[cfg(test)]
mod tests;
