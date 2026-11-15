//! A Turn's session activity rows (hook runs) as append-only session facts
//! owned by that Turn, so they outlive the live session surface: a new
//! runtime epoch or a later Turn's rows never remove them.
//!
//! Each change of a row appends one fact; the projection folds the Turn's
//! facts into its latest row per activity. Caps per Turn: at most
//! [`ROWS_PER_TURN`] rows, and once [`FACTS_PER_TURN`] facts exist only a
//! fact that settles a row (running, or cancelled at the Turn's end, to its
//! outcome) is admitted, up to `FACTS_PER_TURN + ROWS_PER_TURN` facts.
//! Display rows also stop at [`HEADROOM_PERCENT`] of the session's fact
//! capacity, leaving the rest to the writes the session needs. A row that
//! cannot be recorded is dropped, never the batch.

use std::{collections::VecDeque, sync::Mutex};

use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::acp::projection::TurnSessionActivityProjection;
use sage_acp_protocol::{
    contract::{ProjectionRevision, RequiredText},
    intent::{
        ExternalId,
        session_event::{
            SESSION_ACTIVITIES_MAX, SESSION_ACTIVITY_KIND_BYTES, SESSION_ACTIVITY_LABEL_BYTES,
            SESSION_ACTIVITY_PROGRESS_BYTES, SessionActivity, SessionActivityState,
        },
    },
};
use sage_core::limits::LIMITS;

use super::{AdapterStoreOwner, StoreError, facts, history::TurnRow};

const KIND: &str = "sessionActivity";
const ROWS_PER_TURN: usize = SESSION_ACTIVITIES_MAX;
const FACTS_PER_TURN: usize = 64;
const FACTS_PER_TURN_MAX: usize = FACTS_PER_TURN + ROWS_PER_TURN;
const HEADROOM_PERCENT: u64 = 90;
/// One Turn's facts of one kind in order, sought through
/// `idx_session_facts_turn`.
pub(super) const TURN_FACTS_SQL: &str = "SELECT payload_json FROM session_facts
     WHERE session_id=?1 AND turn_id=?2 AND kind=?3 ORDER BY sequence LIMIT ?4";
static WARNED: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
const WARNED_MAX: usize = 32;

/// One recorded state of a row. `i` is the harness's row identity and
/// stays in native storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ActivityFact {
    i: ExternalId,
    k: RequiredText<SESSION_ACTIVITY_KIND_BYTES>,
    l: RequiredText<SESSION_ACTIVITY_LABEL_BYTES>,
    s: SessionActivityState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    p: Option<RequiredText<SESSION_ACTIVITY_PROGRESS_BYTES>>,
}

/// Records `row` in its Turn and reports whether a fact was appended.
pub(super) fn record(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    row: &SessionActivity,
    projection: ProjectionRevision,
    now_millis: i64,
) -> Result<bool, StoreError> {
    let session = owner.session_id.as_str();
    let turn = owning_turn(transaction, owner, row.in_previous_turn)?;
    let (rows, count) = folded(transaction, session, &turn)?;
    let fact = ActivityFact {
        i: row.id.clone(),
        k: row.kind.clone(),
        l: row.label.clone(),
        s: row.state,
        p: row.progress.clone(),
    };
    let admitted = match rows.iter().find(|latest| latest.i == fact.i) {
        Some(latest) if *latest == fact => false,
        Some(latest) => {
            count < FACTS_PER_TURN || (count < FACTS_PER_TURN_MAX && settles(latest.s, fact.s))
        }
        None => count < FACTS_PER_TURN && rows.len() < ROWS_PER_TURN,
    };
    if !admitted {
        return Ok(false);
    }
    let used: i64 = transaction.query_row(
        "SELECT fact_count FROM acp_session_usage WHERE session_id=?1",
        [session],
        |row| row.get(0),
    )?;
    let headroom = LIMITS.durable.facts_per_session * HEADROOM_PERCENT / 100;
    if used >= headroom as i64 {
        warn_once(session, "sessionFactHeadroom");
        return Ok(false);
    }
    let payload = serde_json::to_string(&fact).map_err(|_| StoreError::Corrupt("activity fact"))?;
    match facts::ensure_fact_capacity(transaction, session, payload.len() as i64) {
        Err(StoreError::Capacity { resource, .. }) => {
            warn_once(session, resource);
            return Ok(false);
        }
        result => result?,
    }
    facts::append_fact_payload(
        transaction,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
        session,
        Some(&turn),
        KIND,
        &payload,
        projection,
        now_millis,
    )?;
    Ok(true)
}

/// Whether `next` settles a row: a running one to any end, or one cancelled
/// at its Turn's end to the outcome that arrived later.
fn settles(latest: SessionActivityState, next: SessionActivityState) -> bool {
    use SessionActivityState::{Cancelled, Completed, Failed, Running};
    matches!(
        (latest, next),
        (Running, Completed | Failed | Cancelled) | (Cancelled, Completed | Failed)
    )
}

/// Suppresses repeated dropped-row warnings while the session remains in the
/// bounded warning cache.
fn warn_once(session: &str, resource: &str) {
    let Ok(mut warned) = WARNED.lock() else {
        return;
    };
    if warned.iter().any(|known| known == session) {
        return;
    }
    if warned.len() == WARNED_MAX {
        warned.pop_front();
    }
    warned.push_back(session.to_owned());
    tracing::warn!(resource, "session activity rows are no longer recorded");
}

/// The Turn's rows in first-report order, each at its latest state.
pub(super) fn load(
    connection: &rusqlite::Connection,
    turn: &TurnRow,
) -> Result<Vec<TurnSessionActivityProjection>, StoreError> {
    Ok(folded(connection, &turn.session_id, &turn.id)?
        .0
        .into_iter()
        .map(|row| TurnSessionActivityProjection {
            kind: row.k,
            label: row.l,
            state: row.s,
            progress: row.p,
        })
        .collect())
}

/// The batch's Turn, or the Turn the harness names when that one already
/// ended: the batch's Turn itself if it is terminal, else the Turn before it
/// in the same runtime epoch. Without such a Turn the batch's Turn owns it.
/// This assumes each Sage Turn of an epoch is exactly one harness Turn, so
/// the harness's previous Turn is the Sage Turn before the batch's.
fn owning_turn(
    transaction: &Transaction<'_>,
    owner: &AdapterStoreOwner,
    previous: bool,
) -> Result<String, StoreError> {
    let current = owner.turn_id.as_str();
    if !previous {
        return Ok(current.to_owned());
    }
    let (ordinal, state): (i64, String) = transaction.query_row(
        "SELECT ordinal,state FROM turns WHERE session_id=?1 AND id=?2",
        params![owner.session_id.as_str(), current],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if super::send::parse_turn_state(&state)?.is_terminal() {
        return Ok(current.to_owned());
    }
    let earlier = transaction
        .query_row(
            "SELECT id FROM turns WHERE session_id=?1 AND runtime_epoch=?2 AND ordinal<?3
             ORDER BY ordinal DESC LIMIT 1",
            params![
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64,
                ordinal
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(earlier.unwrap_or_else(|| current.to_owned()))
}

/// Folds a Turn's facts and counts them. A fact that does not decode (one
/// written by a newer build, say) is skipped, so it never blocks the page.
fn folded(
    connection: &rusqlite::Connection,
    session: &str,
    turn: &str,
) -> Result<(Vec<ActivityFact>, usize), StoreError> {
    let mut statement = connection.prepare_cached(TURN_FACTS_SQL)?;
    let limit = FACTS_PER_TURN_MAX as i64;
    let payloads = statement
        .query_map(params![session, turn, KIND, limit], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let count = payloads.len();
    let mut rows: Vec<ActivityFact> = Vec::new();
    for payload in payloads {
        let Ok(fact) = serde_json::from_str::<ActivityFact>(&payload) else {
            tracing::warn!("skipped a session activity fact that does not decode");
            continue;
        };
        if let Some(row) = rows.iter_mut().find(|row| row.i == fact.i) {
            *row = fact;
        } else if rows.len() < ROWS_PER_TURN {
            rows.push(fact);
        }
    }
    Ok((rows, count))
}
