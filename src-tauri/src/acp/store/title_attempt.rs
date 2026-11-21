//! Bounded title state in the immutable fact ledger. Only the latest fact is
//! authoritative: at most one pending retry per chat, and three admitted tries.
//! Busy admission adds at most one initial pending fact. Each admitted try adds
//! a claim and at most one preemption fact (at most six facts for the chat).
//! Claims fail closed on errors or process exit; only foreground preemption
//! explicitly returns a claim to pending.
use rusqlite::{Connection, OptionalExtension, params};
use sage_acp_protocol::{contract::SageTurnId, scheduler::RuntimeOwner};

use super::{StoreError, facts};

const CLAIM: &str = "chatTitleAttempt";
// Every title marker is anchored to the first exchange. This lookup seeks
// idx_session_facts_turn rather than scanning the whole Chat fact history.
pub(super) const LATEST_SQL: &str =
    "SELECT session_id,json_extract(payload_json,'$.state'),json_extract(payload_json,'$.token'),
         COALESCE(json_extract(payload_json,'$.tries'),3) FROM session_facts
         WHERE session_id=?1 AND turn_id=?2 AND kind=?3 ORDER BY sequence DESC LIMIT 1";

mod pending;
pub(crate) use pending::pending_owner;

pub(crate) struct TitleAttempt {
    pub(crate) owner: RuntimeOwner,
    pub(crate) anchor: SageTurnId,
    completed_turn: SageTurnId,
    pub(crate) token: String,
    pub(crate) input: String,
    title: String,
    source: String,
    tries: u8,
}

pub(crate) enum TitleClaim {
    Pending,
    Stopped,
    Started(Box<TitleAttempt>),
}

pub(crate) fn claim(
    connection: &mut Connection,
    owner: RuntimeOwner,
    completed_turn: SageTurnId,
    admitted: bool,
) -> Result<TitleClaim, StoreError> {
    claim_inner(connection, owner, Some(completed_turn), admitted)
}

pub(crate) fn retry(
    connection: &mut Connection,
    owner: RuntimeOwner,
) -> Result<TitleClaim, StoreError> {
    claim_inner(connection, owner, None, true)
}

fn claim_inner(
    connection: &mut Connection,
    owner: RuntimeOwner,
    completed_turn: Option<SageTurnId>,
    admitted: bool,
) -> Result<TitleClaim, StoreError> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let row: Option<(String, String)> = transaction
        .query_row(
            "SELECT title,title_source FROM chats c WHERE project_id=?1 AND id=?2
         AND title_source IN ('default','prompt')
         AND EXISTS(SELECT 1 FROM chat_tabs WHERE project_id=c.project_id AND chat_id=c.id)",
            params![owner.project_id.as_str(), owner.chat_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((title, source)) = row else {
        return Ok(TitleClaim::Stopped);
    };
    let first: Option<String> = transaction
        .query_row(
            concat!(
                "SELECT t.id FROM turns t WHERE t.project_id=?1 AND t.chat_id=?2
          AND t.state='completed' AND ",
                super::prompted_turn!("t.session_id", "t.id"),
                " ORDER BY t.created_at,t.ordinal,t.id LIMIT 1"
            ),
            params![owner.project_id.as_str(), owner.chat_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(first) = first else {
        return Ok(TitleClaim::Stopped);
    };
    let previous = latest(&transaction, &owner, &first)?;
    let tries = match &previous {
        Some(previous) => {
            if previous.session != owner.session_id.as_str()
                || !previous.pending
                || previous.tries >= 3
            {
                return Ok(TitleClaim::Stopped);
            }
            // Repeated busy completions do not append facts or spend tries.
            if !admitted {
                return Ok(TitleClaim::Pending);
            }
            previous.tries
        }
        None => 0,
    };
    if previous.is_none() && completed_turn.as_ref().map(SageTurnId::as_str) != Some(first.as_str())
    {
        return Ok(TitleClaim::Stopped);
    }
    let anchor = SageTurnId::parse(first).map_err(|_| StoreError::Corrupt("title anchor"))?;
    let attempt = TitleAttempt {
        owner,
        completed_turn: completed_turn.unwrap_or_else(|| anchor.clone()),
        anchor,
        token: uuid::Uuid::new_v4().to_string(),
        input: String::new(),
        title,
        source,
        tries: tries + u8::from(admitted),
    };
    if !anchor_valid(&transaction, &attempt)? {
        return Ok(TitleClaim::Stopped);
    }
    // SQLite clips each extracted text before it crosses the native boundary.
    // Only the prompt and last completed assistant text are admitted, never tools/thoughts.
    let text = |predicate: &str, direction: &str| -> Result<Option<String>, StoreError> {
        Ok(transaction
            .query_row(
                &format!(
                    "SELECT substr(json_extract(payload_json,'$.payload.text'),1,2048) FROM items
             WHERE turn_id=?1 AND session_id=?2 AND origin='primary' AND kind='text'
             AND lifecycle='completed' AND {predicate} ORDER BY ordinal {direction} LIMIT 1"
                ),
                params![attempt.anchor.as_str(), attempt.owner.session_id.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    };
    let (Some(user), Some(assistant)) = (text("ordinal=0", "ASC")?, text("ordinal>0", "DESC")?)
    else {
        return Ok(TitleClaim::Stopped);
    };
    let mut attempt = attempt;
    attempt.input = format!(
        "User:\n{}\nAssistant:\n{}",
        prefix(&user, 2000),
        prefix(&assistant, 2000)
    );
    append_state(
        &transaction,
        &attempt,
        if admitted { "claimed" } else { "pending" },
    )?;
    transaction.commit()?;
    Ok(if admitted {
        TitleClaim::Started(Box::new(attempt))
    } else {
        TitleClaim::Pending
    })
}

struct AttemptState {
    session: String,
    pending: bool,
    token: String,
    tries: u8,
}

fn latest(
    connection: &Connection,
    owner: &RuntimeOwner,
    anchor: &str,
) -> Result<Option<AttemptState>, StoreError> {
    Ok(connection
        .query_row(
            LATEST_SQL,
            params![owner.session_id.as_str(), anchor, CLAIM],
            |row| {
                Ok(AttemptState {
                    session: row.get(0)?,
                    pending: row.get::<_, String>(1)? == "pending",
                    token: row.get(2)?,
                    tries: row.get(3)?,
                })
            },
        )
        .optional()?)
}

fn append_state(
    transaction: &rusqlite::Transaction<'_>,
    attempt: &TitleAttempt,
    state: &str,
) -> Result<(), StoreError> {
    let payload =
        serde_json::json!({"state":state,"token":attempt.token,"tries":attempt.tries}).to_string();
    // Claiming is invisible metadata and does not invalidate a racing send.
    let current: i64 = transaction.query_row(
        "SELECT projection_revision FROM chats WHERE id=?1",
        [attempt.owner.chat_id.as_str()],
        |row| row.get(0),
    )?;
    let revision = facts::projection_from_i64(current)?;
    facts::append_fact_payload(
        transaction,
        attempt.owner.project_id.as_str(),
        attempt.owner.chat_id.as_str(),
        attempt.owner.session_id.as_str(),
        Some(attempt.anchor.as_str()),
        CLAIM,
        &payload,
        revision,
        sage_core::time::now_millis(),
    )?;
    Ok(())
}

pub(crate) fn preempt(
    connection: &mut Connection,
    attempt: &TitleAttempt,
) -> Result<bool, StoreError> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let pending = attempt.tries < 3 && valid(&transaction, attempt)?;
    if pending {
        append_state(&transaction, attempt, "pending")?;
    }
    transaction.commit()?;
    Ok(pending)
}

/// A following turn may finish while cancellation is still cleaning up.
/// Its durable completion is the retry signal; no waiting intent is retained.
pub(crate) fn next_completion(
    connection: &Connection,
    attempt: &TitleAttempt,
) -> Result<Option<SageTurnId>, StoreError> {
    let next: Option<String> = connection
        .query_row(
            "SELECT id FROM turns WHERE session_id=?1 AND runtime_epoch=?2 AND state='completed'
         AND ordinal > (SELECT ordinal FROM turns WHERE id=?3)
         ORDER BY ordinal DESC LIMIT 1",
            params![
                attempt.owner.session_id.as_str(),
                attempt.owner.runtime_epoch.get() as i64,
                attempt.completed_turn.as_str()
            ],
            |row| row.get(0),
        )
        .optional()?;
    next.map(|id| SageTurnId::parse(id).map_err(|_| StoreError::Corrupt("title retry turn")))
        .transpose()
}

pub(crate) fn prefix(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

// Runtime ownership belongs to the running epoch. The first exchange belongs
// to the session history and remains eligible after a park and reactivation.
fn anchor_valid(connection: &Connection, attempt: &TitleAttempt) -> Result<bool, StoreError> {
    Ok(connection.query_row(
        concat!(
            "SELECT EXISTS(SELECT 1 FROM chats c
         JOIN chat_session_heads h ON h.project_id=c.project_id AND h.chat_id=c.id
         JOIN agent_sessions s ON s.id=h.session_id
         JOIN session_runtime_epochs e ON e.session_id=s.id
         JOIN turns t ON t.session_id=s.id
         WHERE c.project_id=?1 AND c.id=?2 AND s.id=?3 AND e.runtime_epoch=?4
         AND s.state IN ('ready','active') AND e.state='running'
         AND t.id=?5 AND t.state='completed' AND ",
            super::turn_undo::visible_turn!("t.session_id", "t.id"),
            " AND c.title=?6 AND c.title_source=?7
         AND EXISTS(SELECT 1 FROM chat_tabs WHERE project_id=c.project_id AND chat_id=c.id))"
        ),
        params![
            attempt.owner.project_id.as_str(),
            attempt.owner.chat_id.as_str(),
            attempt.owner.session_id.as_str(),
            attempt.owner.runtime_epoch.get() as i64,
            attempt.anchor.as_str(),
            attempt.title,
            attempt.source
        ],
        |row| row.get(0),
    )?)
}

pub(crate) fn valid(connection: &Connection, attempt: &TitleAttempt) -> Result<bool, StoreError> {
    Ok(anchor_valid(connection, attempt)?
        && latest(connection, &attempt.owner, attempt.anchor.as_str())?.is_some_and(|state| {
            state.session == attempt.owner.session_id.as_str()
                && !state.pending
                && state.token == attempt.token
        }))
}

pub(crate) fn apply(
    connection: &mut Connection,
    attempt: &TitleAttempt,
    raw: &str,
) -> Result<Option<sage_acp_protocol::contract::ProjectionRevision>, StoreError> {
    let Some(title) = crate::chat_lifecycle::generated_title(raw) else {
        return Ok(None);
    };
    apply_existing(connection, attempt, &title)
}

pub(crate) fn apply_existing(
    connection: &mut Connection,
    attempt: &TitleAttempt,
    raw: &str,
) -> Result<Option<sage_acp_protocol::contract::ProjectionRevision>, StoreError> {
    let Some(title) = crate::chat_lifecycle::harness_title(raw) else {
        return Ok(None);
    };
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if !valid(&transaction, attempt)? {
        return Ok(None);
    }
    let now = sage_core::time::now_millis();
    crate::chat_lifecycle::apply_harness_title_in(
        &transaction,
        attempt.owner.project_id.as_str(),
        attempt.owner.chat_id.as_str(),
        crate::chat_lifecycle::chat_display_title(&title),
        now,
    )?;
    let revision = facts::next_projection_revision(
        &transaction,
        attempt.owner.project_id.as_str(),
        attempt.owner.chat_id.as_str(),
    )?;
    facts::publish_send_neutral_projection(
        &transaction,
        attempt.owner.project_id.as_str(),
        attempt.owner.chat_id.as_str(),
        attempt.owner.session_id.as_str(),
        revision,
        now,
    )?;
    transaction.commit()?;
    Ok(Some(revision))
}
