use rusqlite::{OptionalExtension, params};

use crate::acp::projection::BrowserOperationProjection;

use super::{StoreError, history::TurnRow};

mod validation;
mod write;
pub(crate) use write::{finish, recover_started, start};

#[cfg(test)]
mod tests;

/// A Turn's uncollapsed operations, newest first. Every fact here is keyed by
/// its Turn, so `idx_session_facts_turn` seeks the Turn's starts and markers
/// instead of scanning the session.
const TURN_OPERATIONS_SQL: &str =
    "SELECT start.sequence,start.payload_json,start.payload_bytes,start.payload_digest
     FROM session_facts start WHERE start.project_id=?1 AND start.chat_id=?2
     AND start.session_id=?3 AND start.turn_id=?4 AND start.kind='browserOperation'
     AND NOT EXISTS (SELECT 1 FROM session_facts marker
       WHERE marker.session_id=start.session_id AND marker.turn_id=start.turn_id
       AND marker.kind='browserOperationsCollapsed'
       AND json_extract(marker.payload_json,'$.s')=start.sequence)
     ORDER BY start.sequence DESC LIMIT 33";

/// The terminal fact of one operation; it carries its start's Turn.
const TERMINAL_SQL: &str = "SELECT payload_json,payload_bytes,payload_digest FROM session_facts
     WHERE session_id=?1 AND turn_id=?2 AND kind='browserOperationTerminal'
     AND json_extract(payload_json,'$.s')=?3 LIMIT 1";

pub(super) fn load(
    connection: &rusqlite::Connection,
    turn: &TurnRow,
) -> Result<(Vec<BrowserOperationProjection>, u64), StoreError> {
    let mut statement = connection.prepare(TURN_OPERATIONS_SQL)?;
    let values = statement
        .query_map(
            params![turn.project_id, turn.chat_id, turn.session_id, turn.id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() > 32 {
        return Err(StoreError::Corrupt("browser operations per turn"));
    }
    let mut operations = values
        .into_iter()
        .map(|(sequence, value, bytes, digest)| {
            if bytes != value.len() as i64 || digest != super::facts::hex_digest(value.as_bytes()) {
                return Err(StoreError::Corrupt("browser operation fact integrity"));
            }
            let mut record = serde_json::from_str::<BrowserOperationProjection>(&value)
                .map_err(|_| StoreError::Corrupt("browser operation fact"))?;
            if record.project_id.as_str() != turn.project_id.as_str()
                || record.chat_id.as_str() != turn.chat_id.as_str()
                || record.session_id.as_str() != turn.session_id.as_str()
                || record.turn_id.as_str() != turn.id.as_str()
            {
                return Err(StoreError::Corrupt("browser operation owner"));
            }
            let terminal: Option<(String, i64, String)> = connection
                .query_row(
                    TERMINAL_SQL,
                    params![turn.session_id, turn.id, sequence],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            if let Some((payload, bytes, digest)) = terminal {
                if bytes != payload.len() as i64
                    || digest != super::facts::hex_digest(payload.as_bytes())
                {
                    return Err(StoreError::Corrupt("browser terminal fact integrity"));
                }
                let terminal: write::TerminalFact = serde_json::from_str(&payload)
                    .map_err(|_| StoreError::Corrupt("browser terminal fact"))?;
                if terminal.s != sequence
                    || terminal.t < record.created_at
                    || !matches!(
                        terminal.p.as_str(),
                        "completed" | "denied" | "failed" | "outcomeUnknown"
                    )
                    || terminal.r.len() > 512
                {
                    return Err(StoreError::Corrupt("browser terminal fact"));
                }
                record.phase = terminal.p;
                record.result_summary = Some(terminal.r);
                record.updated_at = terminal.t;
            }
            Ok(record)
        })
        .collect::<Result<Vec<_>, _>>()?;
    operations.reverse();
    let collapsed: Option<String> = connection
        .query_row(
            "SELECT json_object('count',COALESCE(SUM(json_extract(payload_json,'$.count')),0))
         FROM session_facts WHERE project_id=?1 AND chat_id=?2
             AND session_id=?3 AND turn_id=?4 AND kind='browserOperationsCollapsed'",
            params![turn.project_id, turn.chat_id, turn.session_id, turn.id],
            |row| row.get(0),
        )
        .optional()?;
    let count = collapsed
        .map(|value| serde_json::from_str::<serde_json::Value>(&value))
        .transpose()
        .map_err(|_| StoreError::Corrupt("browser collapsed fact"))?
        .and_then(|value| value.get("count").and_then(serde_json::Value::as_u64))
        .unwrap_or(0);
    Ok((operations, count))
}
