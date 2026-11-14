use rusqlite::{Transaction, params};
use sage_db::error::SqliteResultExt;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use sage_core::error::{AppError, AppResult};

use super::{
    identity::{hex_digest, legacy_turn_id},
    legacy::LegacySession,
    limits::{BYTES_PER_PAGE, BYTES_PER_TURN, ITEM_BYTES, ITEMS_PER_TURN, ROWS_PER_PAGE},
};

#[derive(Debug)]
struct LegacyEvent {
    id: String,
    sequence: i64,
    payload: String,
    created_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyMessage {
    role: String,
    text: String,
}

pub(super) fn convert_session_events(
    transaction: &Transaction<'_>,
    session: &LegacySession,
) -> AppResult<()> {
    let mut after_sequence = 0_i64;
    let mut observed = 0_i64;
    let mut turn_ordinal = 0_i64;
    let mut chunk = Vec::with_capacity(ITEMS_PER_TURN);
    let mut chunk_bytes = 0_usize;
    loop {
        let page = event_page(transaction, &session.id, after_sequence)?;
        if page.is_empty() {
            break;
        }
        for event in page {
            if event.sequence != observed + 1 {
                return invalid("legacy event sequence is not exactly positive and contiguous");
            }
            validate_event(&event)?;
            if !chunk.is_empty()
                && (chunk.len() == ITEMS_PER_TURN
                    || chunk_bytes.saturating_add(event.payload.len()) > BYTES_PER_TURN)
            {
                write_turn(transaction, session, turn_ordinal, &chunk)?;
                turn_ordinal += 1;
                chunk.clear();
                chunk_bytes = 0;
            }
            chunk_bytes = chunk_bytes.saturating_add(event.payload.len());
            after_sequence = event.sequence;
            observed = event.sequence;
            chunk.push(event);
        }
    }
    if !chunk.is_empty() {
        write_turn(transaction, session, turn_ordinal, &chunk)?;
    }
    if observed != session.last_sequence {
        return invalid("legacy last_committed_sequence does not match event history");
    }
    Ok(())
}

fn event_page(
    transaction: &Transaction<'_>,
    session_id: &str,
    after_sequence: i64,
) -> AppResult<Vec<LegacyEvent>> {
    let mut statement = transaction
        .prepare(
            "SELECT id, sequence, payload_json, created_at
         FROM legacy_session_events_v4
         WHERE session_id=?1 AND sequence>?2
         ORDER BY sequence LIMIT ?3",
        )
        .db()?;
    let mut rows = statement
        .query(params![
            session_id,
            after_sequence,
            ROWS_PER_PAGE as i64 + 1
        ])
        .db()?;
    let mut page = Vec::with_capacity(ROWS_PER_PAGE);
    let mut bytes = 0_usize;
    while let Some(row) = rows.next().db()? {
        if page.len() == ROWS_PER_PAGE {
            break;
        }
        let event = LegacyEvent {
            id: row.get(0).db()?,
            sequence: row.get(1).db()?,
            payload: row.get(2).db()?,
            created_at: row.get(3).db()?,
        };
        let row_bytes = event.id.len().saturating_add(event.payload.len());
        if !page.is_empty() && bytes.saturating_add(row_bytes) > BYTES_PER_PAGE {
            break;
        }
        bytes = bytes.saturating_add(row_bytes);
        page.push(event);
    }
    Ok(page)
}

fn validate_event(event: &LegacyEvent) -> AppResult<()> {
    if !(1..=512).contains(&event.id.len()) || event.payload.len() > ITEM_BYTES {
        return Err(AppError::ResourceLimit(
            "legacy event violates its ID or Item byte limit".into(),
        ));
    }
    let message: LegacyMessage = serde_json::from_str(&event.payload)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    if !matches!(message.role.as_str(), "user" | "assistant" | "system") {
        return invalid("legacy message has an unsupported role");
    }
    if message.text.len() > ITEM_BYTES {
        return Err(AppError::ResourceLimit(
            "legacy message text exceeds the normalized Item byte limit".into(),
        ));
    }
    Ok(())
}

fn write_turn(
    transaction: &Transaction<'_>,
    session: &LegacySession,
    ordinal: i64,
    events: &[LegacyEvent],
) -> AppResult<()> {
    let first = events.first().expect("legacy Turn is nonempty");
    let last = events.last().expect("legacy Turn is nonempty");
    let turn_id = legacy_turn_id(&session.id, first.sequence, last.sequence);
    transaction
        .execute(
            "INSERT INTO turns (
           id, project_id, chat_id, session_id, runtime_epoch, ordinal,
           client_intent_id, state, delivery_state, revision,
           created_at, updated_at, terminal_at
         ) VALUES (?1,?2,?3,?4,0,?5,?6,'completed','correlated',0,?7,?8,?8)",
            params![
                turn_id,
                session.project_id,
                session.chat_tab_id,
                session.id,
                ordinal,
                format!("legacy-{ordinal}"),
                first.created_at,
                last.created_at
            ],
        )
        .db()?;
    let mut aggregate = Sha256::new();
    for event in events {
        let digest = hex_digest(event.payload.as_bytes());
        aggregate.update(digest.as_bytes());
        transaction
            .execute(
                "INSERT INTO items (
               id, project_id, chat_id, session_id, turn_id, runtime_epoch,
               ordinal, origin, activity_id, kind, lifecycle, tool_state,
               external_item_id, payload_json, payload_bytes, payload_digest,
               revision, created_at, updated_at, terminal_at
             ) VALUES (
               ?1,?2,?3,?4,?5,0,?6,'primary',NULL,'text','completed',NULL,
               NULL,?7,?8,?9,0,?10,?10,?10
             )",
                params![
                    event.id,
                    session.project_id,
                    session.chat_tab_id,
                    session.id,
                    turn_id,
                    event.sequence,
                    event.payload,
                    event.payload.len() as i64,
                    digest,
                    event.created_at
                ],
            )
            .db()?;
    }
    let fact_payload = json!({
        "firstSequence": first.sequence,
        "lastSequence": last.sequence,
        "itemCount": events.len(),
        "aggregateDigest": format!("{:x}", aggregate.finalize())
    })
    .to_string();
    transaction
        .execute(
            "INSERT INTO session_facts (
           project_id, chat_id, session_id, sequence, turn_id, kind,
           payload_json, payload_bytes, payload_digest,
           projection_revision, created_at
         ) VALUES (?1,?2,?3,?4,?5,'legacyTurnRange',?6,?7,?8,?4,?9)",
            params![
                session.project_id,
                session.chat_tab_id,
                session.id,
                ordinal + 1,
                turn_id,
                fact_payload,
                fact_payload.len() as i64,
                hex_digest(fact_payload.as_bytes()),
                last.created_at
            ],
        )
        .db()?;
    Ok(())
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}
