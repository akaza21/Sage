use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::acp::{contract::RuntimeEpoch, projection::BrowserOperationProjection};

use super::super::{
    StoreError,
    facts::{
        append_fact_payload, ensure_fact_capacity, next_projection_revision, publish_projection,
    },
};
use super::validation::validate;

const PER_TURN: i64 = 32;
const PER_CHAT: i64 = 128;
// One future append-only terminal fact is charged at start, then spent at finish.
pub(super) const TERMINAL_RESERVE_BYTES: usize = 600;

#[derive(Serialize, Deserialize)]
pub(super) struct TerminalFact {
    pub(super) s: i64,
    pub(super) p: String,
    pub(super) r: String,
    pub(super) t: i64,
}

pub(crate) fn start(
    connection: &mut Connection,
    epoch: RuntimeEpoch,
    record: &BrowserOperationProjection,
) -> Result<(), StoreError> {
    validate(record)?;
    if record.phase != "started" || record.result_summary.is_some() {
        return Err(StoreError::InvalidTransition {
            resource: "browserOperation",
        });
    }
    let tx = connection.transaction()?;
    let live: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM turns t JOIN agent_sessions s ON s.id=t.session_id
         JOIN session_runtime_epochs e ON e.session_id=t.session_id AND e.runtime_epoch=t.runtime_epoch
         WHERE t.project_id=?1 AND t.chat_id=?2 AND t.session_id=?3 AND t.id=?4
           AND t.runtime_epoch=?5 AND t.state NOT IN ('completed','interrupted','cancelled','failed','deliveryUnknown')
           AND s.state IN ('ready','active') AND e.state='running')",
        params![record.project_id.as_str(), record.chat_id.as_str(), record.session_id.as_str(),
            record.turn_id.as_str(), epoch.get() as i64],
        |row| row.get(0),
    )?;
    if !live {
        return Err(StoreError::Conflict {
            resource: "browserOperationOwner",
        });
    }
    let duplicate: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_facts WHERE session_id=?1 AND kind='browserOperation'
         AND json_extract(payload_json,'$.operationId')=?2)",
        params![record.session_id.as_str(), record.operation_id],
        |row| row.get(0),
    )?;
    if duplicate {
        return Err(StoreError::Conflict {
            resource: "browserOperationId",
        });
    }
    // Every browser operation start, and so every collapse marker copied from
    // one, carries its Turn, so `marker.turn_id=start.turn_id` never meets a
    // NULL. Plain equality also lets these reads use the partial Turn index.
    let turn_count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM session_facts start WHERE start.session_id=?1 AND start.turn_id=?2
         AND start.kind='browserOperation' AND NOT EXISTS (
           SELECT 1 FROM session_facts marker WHERE marker.session_id=start.session_id
           AND marker.turn_id=start.turn_id AND marker.kind='browserOperationsCollapsed'
           AND json_extract(marker.payload_json,'$.s')=start.sequence)",
        params![record.session_id.as_str(), record.turn_id.as_str()],
        |row| row.get(0),
    )?;
    let chat_count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM session_facts start WHERE start.project_id=?1 AND start.chat_id=?2
         AND start.kind='browserOperation' AND NOT EXISTS (
           SELECT 1 FROM session_facts marker WHERE marker.session_id=start.session_id
           AND marker.turn_id=start.turn_id AND marker.kind='browserOperationsCollapsed'
           AND json_extract(marker.payload_json,'$.s')=start.sequence)",
        params![record.project_id.as_str(), record.chat_id.as_str()],
        |row| row.get(0),
    )?;
    let revision =
        next_projection_revision(&tx, record.project_id.as_str(), record.chat_id.as_str())?;
    if turn_count >= PER_TURN {
        collapse_oldest(&tx, record, Some(record.turn_id.as_str()), revision)?;
    } else if chat_count >= PER_CHAT {
        collapse_oldest(&tx, record, None, revision)?;
    }
    let payload = serde_json::to_string(record)
        .map_err(|_| StoreError::Corrupt("browser operation serialize"))?;
    append_fact_payload(
        &tx,
        record.project_id.as_str(),
        record.chat_id.as_str(),
        record.session_id.as_str(),
        Some(record.turn_id.as_str()),
        "browserOperation",
        &payload,
        revision,
        record.created_at,
    )?;
    ensure_fact_capacity(
        &tx,
        record.session_id.as_str(),
        TERMINAL_RESERVE_BYTES as i64,
    )?;
    adjust_usage(
        &tx,
        record.project_id.as_str(),
        record.session_id.as_str(),
        1,
        TERMINAL_RESERVE_BYTES as i64,
    )?;
    publish_projection(
        &tx,
        record.project_id.as_str(),
        record.chat_id.as_str(),
        record.session_id.as_str(),
        revision,
        record.created_at,
    )?;
    tx.commit().map_err(|_| StoreError::CommitUnknown)
}

pub(crate) fn finish(
    connection: &mut Connection,
    record: &BrowserOperationProjection,
) -> Result<(), StoreError> {
    validate(record)?;
    if record.phase == "started" {
        return Err(StoreError::InvalidTransition {
            resource: "browserOperation",
        });
    }
    let tx = connection.transaction()?;
    let row: Option<(i64, String)> = tx
        .query_row(
            "SELECT sequence,payload_json FROM session_facts
         WHERE project_id=?1 AND chat_id=?2 AND session_id=?3 AND turn_id=?4
         AND kind='browserOperation' AND json_extract(payload_json,'$.operationId')=?5 LIMIT 1",
            params![
                record.project_id.as_str(),
                record.chat_id.as_str(),
                record.session_id.as_str(),
                record.turn_id.as_str(),
                record.operation_id
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((sequence, previous)) = row else {
        return Err(StoreError::Conflict {
            resource: "browserOperation",
        });
    };
    let old: BrowserOperationProjection = serde_json::from_str(&previous)
        .map_err(|_| StoreError::Corrupt("browser operation fact"))?;
    if old.phase != "started"
        || old.operation != record.operation
        || old.tab_id != record.tab_id
        || old.origin != record.origin
        || old.summary != record.summary
        || old.created_at != record.created_at
    {
        return Err(StoreError::InvalidTransition {
            resource: "browserOperation",
        });
    }
    let already_terminal: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_facts WHERE session_id=?1
         AND kind='browserOperationTerminal' AND json_extract(payload_json,'$.s')=?2)",
        params![record.session_id.as_str(), sequence],
        |row| row.get(0),
    )?;
    if already_terminal {
        return Err(StoreError::Conflict {
            resource: "browserOperation",
        });
    }
    let revision =
        next_projection_revision(&tx, record.project_id.as_str(), record.chat_id.as_str())?;
    append_terminal(&tx, record, sequence, revision)?;
    publish_projection(
        &tx,
        record.project_id.as_str(),
        record.chat_id.as_str(),
        record.session_id.as_str(),
        revision,
        record.updated_at,
    )?;
    tx.commit().map_err(|_| StoreError::CommitUnknown)
}

/// One keyset page of starts that never reached a terminal fact. The page walks
/// `idx_session_facts_operation_start`, which holds only starts, and each start
/// seeks its own terminal through `idx_session_facts_operation_terminal`, so a
/// restart reads neither other facts nor finished operations' terminals.
/// The unary `+` drops `start.sequence`'s INTEGER column affinity; with it the
/// comparison would convert the `$.s` key and could no longer seek the index.
pub(super) const RECOVER_SQL: &str = "SELECT start.session_id,start.sequence,start.payload_json
     FROM session_facts start
     WHERE (start.session_id,start.sequence) > (?1,?2) AND start.kind='browserOperation'
       AND NOT EXISTS (SELECT 1 FROM session_facts terminal
         WHERE terminal.session_id=start.session_id
         AND terminal.kind='browserOperationTerminal'
         AND json_extract(terminal.payload_json,'$.s')=+start.sequence)
     ORDER BY start.session_id,start.sequence LIMIT 512";

pub(crate) fn recover_started(connection: &mut Connection, now: i64) -> Result<u64, StoreError> {
    let tx = connection.transaction()?;
    let mut cursor_session = String::new();
    let mut cursor_sequence = 0_i64;
    let mut recovered = 0_u64;
    loop {
        let rows = {
            let mut statement = tx.prepare_cached(RECOVER_SQL)?;
            statement
                .query_map(params![cursor_session, cursor_sequence], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        if rows.is_empty() {
            break;
        }
        for (session_id, sequence, payload) in rows {
            cursor_session = session_id;
            cursor_sequence = sequence;
            let mut record: BrowserOperationProjection = serde_json::from_str(&payload)
                .map_err(|_| StoreError::Corrupt("browser operation fact"))?;
            record.phase = "outcomeUnknown".into();
            record.result_summary =
                Some("Sage restarted before the browser result was known".into());
            record.updated_at = now.max(record.created_at);
            let revision =
                next_projection_revision(&tx, record.project_id.as_str(), record.chat_id.as_str())?;
            append_terminal(&tx, &record, sequence, revision)?;
            publish_projection(
                &tx,
                record.project_id.as_str(),
                record.chat_id.as_str(),
                record.session_id.as_str(),
                revision,
                now,
            )?;
            recovered += 1;
        }
    }
    tx.commit().map_err(|_| StoreError::CommitUnknown)?;
    Ok(recovered)
}

fn append_terminal(
    tx: &Transaction<'_>,
    record: &BrowserOperationProjection,
    sequence: i64,
    revision: crate::acp::contract::ProjectionRevision,
) -> Result<(), StoreError> {
    let mut terminal = TerminalFact {
        s: sequence,
        p: record.phase.clone(),
        r: record.result_summary.clone().unwrap_or_default(),
        t: record.updated_at,
    };
    let mut payload = serde_json::to_string(&terminal)
        .map_err(|_| StoreError::Corrupt("browser terminal serialize"))?;
    if payload.len() > TERMINAL_RESERVE_BYTES {
        terminal.r = if record.phase == "outcomeUnknown" {
            "Outcome unknown; inspect page before retrying".into()
        } else {
            "Browser operation finished".into()
        };
        payload = serde_json::to_string(&terminal)
            .map_err(|_| StoreError::Corrupt("browser terminal serialize"))?;
    }
    if payload.len() > TERMINAL_RESERVE_BYTES {
        return Err(StoreError::Corrupt("browser terminal reservation exceeded"));
    }
    // The start charged one fact slot and 600 bytes in advance. Return those
    // reserved counters inside this transaction, then append the real fact.
    adjust_usage(
        tx,
        record.project_id.as_str(),
        record.session_id.as_str(),
        -1,
        -(TERMINAL_RESERVE_BYTES as i64),
    )?;
    append_fact_payload(
        tx,
        record.project_id.as_str(),
        record.chat_id.as_str(),
        record.session_id.as_str(),
        Some(record.turn_id.as_str()),
        "browserOperationTerminal",
        &payload,
        revision,
        record.updated_at,
    )
}

fn collapse_oldest(
    tx: &Transaction<'_>,
    record: &BrowserOperationProjection,
    turn: Option<&str>,
    revision: crate::acp::contract::ProjectionRevision,
) -> Result<(), StoreError> {
    let row: (String, i64, String) = tx.query_row(
        "SELECT start.session_id,start.sequence,start.turn_id FROM session_facts start
         WHERE start.project_id=?1 AND start.chat_id=?2 AND start.kind='browserOperation'
           AND (?3 IS NULL OR start.turn_id=?3)
           AND NOT EXISTS (SELECT 1 FROM session_facts marker
             WHERE marker.session_id=start.session_id AND marker.turn_id=start.turn_id
             AND marker.kind='browserOperationsCollapsed'
             AND json_extract(marker.payload_json,'$.s')=start.sequence)
         ORDER BY start.created_at,start.session_id,start.sequence LIMIT 1",
        params![record.project_id.as_str(), record.chat_id.as_str(), turn],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    append_fact_payload(
        tx,
        record.project_id.as_str(),
        record.chat_id.as_str(),
        &row.0,
        Some(&row.2),
        "browserOperationsCollapsed",
        &serde_json::json!({"s":row.1,"count":1}).to_string(),
        revision,
        record.created_at,
    )
}

fn adjust_usage(
    tx: &Transaction<'_>,
    project: &str,
    session: &str,
    count: i64,
    bytes: i64,
) -> Result<(), StoreError> {
    tx.execute(
        "UPDATE acp_session_usage SET fact_count=fact_count+?2,fact_bytes=fact_bytes+?3,
        payload_bytes=payload_bytes+?3,revision=revision+1 WHERE session_id=?1",
        params![session, count, bytes],
    )?;
    tx.execute(
        "UPDATE acp_project_usage SET fact_count=fact_count+?2,payload_bytes=payload_bytes+?3,
        revision=revision+1 WHERE project_id=?1",
        params![project, count, bytes],
    )?;
    tx.execute(
        "UPDATE acp_store_usage SET fact_count=fact_count+?1,fact_bytes=fact_bytes+?2,
        shared_payload_bytes=shared_payload_bytes+?2,revision=revision+1 WHERE singleton=1",
        params![count, bytes],
    )?;
    Ok(())
}
