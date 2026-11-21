//! One immutable claim per chat, using the existing bounded fact ledger.
//! A claimed request is spent even if its process or Sage exits before a result.
use rusqlite::{Connection, OptionalExtension, params};
use sage_acp_protocol::{contract::SageTurnId, scheduler::RuntimeOwner};

use super::{StoreError, facts};

const CLAIM: &str = "chatTitleAttempt";

pub(crate) struct TitleAttempt {
    pub(crate) owner: RuntimeOwner,
    pub(crate) anchor: SageTurnId,
    pub(crate) token: String,
    pub(crate) input: String,
    title: String,
    source: String,
}

pub(crate) fn claim(
    connection: &mut Connection,
    owner: RuntimeOwner,
    anchor: SageTurnId,
) -> Result<Option<TitleAttempt>, StoreError> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let row: Option<(String, String)> = transaction
        .query_row(
            "SELECT title,title_source FROM chats c WHERE project_id=?1 AND id=?2
         AND title_source IN ('default','prompt')
         AND EXISTS(SELECT 1 FROM chat_tabs WHERE project_id=c.project_id AND chat_id=c.id)
         AND NOT EXISTS(SELECT 1 FROM session_facts WHERE project_id=c.project_id
           AND chat_id=c.id AND kind='chatTitleAttempt')",
            params![owner.project_id.as_str(), owner.chat_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((title, source)) = row else {
        return Ok(None);
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
    if first.as_deref() != Some(anchor.as_str()) {
        return Ok(None);
    }
    let attempt = TitleAttempt {
        owner,
        anchor,
        token: uuid::Uuid::new_v4().to_string(),
        input: String::new(),
        title,
        source,
    };
    if !anchor_valid(&transaction, &attempt)? {
        return Ok(None);
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
        return Ok(None);
    };
    let mut attempt = attempt;
    attempt.input = format!(
        "User:\n{}\nAssistant:\n{}",
        prefix(&user, 2000),
        prefix(&assistant, 2000)
    );
    let payload = serde_json::json!({"state":"claimed","token":attempt.token}).to_string();
    // Claiming is invisible metadata and does not invalidate a racing send.
    let current: i64 = transaction.query_row(
        "SELECT projection_revision FROM chats WHERE id=?1",
        [attempt.owner.chat_id.as_str()],
        |row| row.get(0),
    )?;
    let revision = facts::projection_from_i64(current)?;
    facts::append_fact_payload(
        &transaction,
        attempt.owner.project_id.as_str(),
        attempt.owner.chat_id.as_str(),
        attempt.owner.session_id.as_str(),
        Some(attempt.anchor.as_str()),
        CLAIM,
        &payload,
        revision,
        sage_core::time::now_millis(),
    )?;
    transaction.commit()?;
    Ok(Some(attempt))
}

pub(crate) fn prefix(text: &str, bytes: usize) -> &str {
    let mut end = text.len().min(bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn anchor_valid(connection: &Connection, attempt: &TitleAttempt) -> Result<bool, StoreError> {
    Ok(connection.query_row(
        concat!(
            "SELECT EXISTS(SELECT 1 FROM chats c
         JOIN chat_session_heads h ON h.project_id=c.project_id AND h.chat_id=c.id
         JOIN agent_sessions s ON s.id=h.session_id
         JOIN session_runtime_epochs e ON e.session_id=s.id
         JOIN turns t ON t.session_id=s.id AND t.runtime_epoch=e.runtime_epoch
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
        && connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_facts WHERE project_id=?1 AND chat_id=?2
         AND session_id=?3 AND turn_id=?4 AND kind=?5 AND json_extract(payload_json,'$.token')=?6)",
            params![
                attempt.owner.project_id.as_str(),
                attempt.owner.chat_id.as_str(),
                attempt.owner.session_id.as_str(),
                attempt.anchor.as_str(),
                CLAIM,
                attempt.token
            ],
            |row| row.get::<_, bool>(0),
        )?)
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
