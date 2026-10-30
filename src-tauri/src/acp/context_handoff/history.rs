// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
use super::Omissions;
use crate::acp::store::StoreError;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::collections::BTreeSet;

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;

pub(crate) struct HistoricalItem {
    pub id: String,
    pub position: i64,
    pub role: String,
    pub is_text: bool,
    pub text: String,
}

pub(crate) struct HistorySnapshot {
    pub watermark: i64,
    pub first_position: Option<i64>,
    pub total_items: u64,
    pub items: Vec<HistoricalItem>,
    pub omissions: Omissions,
}

pub(crate) fn watermark(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<i64, StoreError> {
    Ok(connection.query_row(
        "SELECT COALESCE(MAX(rowid),0) FROM items WHERE project_id=?1 AND chat_id=?2",
        params![project, chat],
        |row| row.get(0),
    )?)
}

pub(crate) fn load(
    connection: &Connection,
    project: &str,
    chat: &str,
    watermark: i64,
) -> Result<HistorySnapshot, StoreError> {
    let (first_position, total_items): (Option<i64>, u64) = connection.query_row(
        "SELECT MIN(rowid),COUNT(*) FROM items WHERE project_id=?1 AND chat_id=?2 AND rowid<=?3",
        params![project, chat, watermark],
        |row| Ok((row.get(0)?, row.get::<_, i64>(1)?.max(0) as u64)),
    )?;
    let mut history = HistorySnapshot {
        watermark,
        first_position,
        total_items,
        items: Vec::new(),
        omissions: Omissions::default(),
    };
    let mut positions = BTreeSet::new();
    for (role, direction) in [("user", "DESC"), ("assistant", "DESC"), ("user", "ASC")] {
        let sql = format!("SELECT rowid FROM items WHERE project_id=?1 AND chat_id=?2 AND kind='text'
            AND CASE WHEN json_extract(payload_json,'$.payload.role')='user' THEN 'user' ELSE 'assistant' END=?3
            AND rowid<=?4 ORDER BY rowid {direction} LIMIT 1");
        if let Some(position) = connection
            .query_row(&sql, params![project, chat, role, watermark], |row| {
                row.get::<_, i64>(0)
            })
            .optional()?
        {
            positions.insert(position);
        }
    }
    let mut seen = BTreeSet::new();
    let mut bytes = 0_usize;
    for position in positions {
        read_item(connection, position, &mut history, &mut seen, &mut bytes)?;
    }
    let mut before = watermark.saturating_add(1);
    while seen.len() < 512 && bytes < 1_048_576 {
        let mut statement = connection.prepare("SELECT rowid FROM items WHERE project_id=?1 AND chat_id=?2 AND rowid<?3 ORDER BY rowid DESC LIMIT 64")?;
        let page = statement
            .query_map(params![project, chat, before], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if page.is_empty() {
            break;
        }
        for position in page {
            before = position;
            if seen.len() == 512 || bytes == 1_048_576 {
                break;
            }
            read_item(connection, position, &mut history, &mut seen, &mut bytes)?;
        }
    }
    history.omissions.scan_limit = total_items.saturating_sub(seen.len() as u64);
    history.items.sort_by_key(|item| item.position);
    Ok(history)
}

fn read_item(
    connection: &Connection,
    position: i64,
    history: &mut HistorySnapshot,
    seen: &mut BTreeSet<i64>,
    bytes: &mut usize,
) -> Result<(), StoreError> {
    if seen.contains(&position) {
        return Ok(());
    }
    let remaining = 1_048_576 - *bytes;
    if remaining < 1_024 {
        *bytes = 1_048_576;
        return Ok(());
    }
    let (id, kind, tool_state, payload, size): (String, String, Option<String>, Option<String>, usize) = connection.query_row(
        "SELECT id,kind,tool_state,CASE WHEN payload_bytes<=?2 AND kind IN ('text','toolCall','fileChangeDiff','error','plan','planEntries') THEN payload_json ELSE NULL END,payload_bytes FROM items WHERE rowid=?1",
        params![position, remaining.saturating_sub(1_024).min(65_536) as i64], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get::<_, i64>(4)?.max(0) as usize)))?;
    if size <= 65_536 && size.saturating_add(1_024) > remaining {
        *bytes = 1_048_576;
        return Ok(());
    }
    seen.insert(position);
    *bytes += id.len() + kind.len() + tool_state.as_ref().map_or(0, String::len);
    if !matches!(
        kind.as_str(),
        "text" | "toolCall" | "fileChangeDiff" | "error" | "plan" | "planEntries"
    ) {
        history.omissions.unsupported_kind += 1;
        return Ok(());
    }
    let Some(payload) = payload else {
        history.omissions.oversize += 1;
        return Ok(());
    };
    *bytes += payload.len();
    if let Some((role, text)) = historical_message(&payload)? {
        history.items.push(HistoricalItem {
            id,
            position,
            role,
            is_text: kind == "text",
            text: if tool_state.as_deref() == Some("cancelled") {
                format!("Interrupted command result: {text}")
            } else {
                text
            },
        });
    } else {
        history.omissions.unsupported_kind += 1;
    }
    Ok(())
}

fn historical_message(raw: &str) -> Result<Option<(String, String)>, StoreError> {
    let value: Value =
        serde_json::from_str(raw).map_err(|_| StoreError::Corrupt("handoff history payload"))?;
    let payload = value.get("payload").unwrap_or(&value);
    let string = |field: &str| {
        payload
            .get(field)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let text = match value.get("kind").and_then(Value::as_str).unwrap_or("text") {
        "text" => string("text").to_owned(),
        "toolCall"
            if matches!(
                payload.get("category").and_then(Value::as_str),
                Some("command" | "execute")
            ) =>
        {
            let execution = &payload["execution"];
            format!(
                "Command: {}\nExit code: {}\n{}",
                string("inputSummary"),
                execution
                    .get("exitCode")
                    .map_or("unknown".into(), Value::to_string),
                execution
                    .get("outputTail")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| string("outputSummary"))
            )
        }
        "fileChangeDiff" => format!(
            "File change: {}",
            payload
                .pointer("/resource/resource/relativePath")
                .or_else(|| payload.pointer("/resource/relativePath"))
                .and_then(Value::as_str)
                .unwrap_or("unavailable")
        ),
        "error" => string("message").to_owned(),
        "plan" => string("summary").to_owned(),
        "planEntries" => payload
            .get("entries")
            .map_or(String::new(), Value::to_string),
        _ => return Ok(None),
    };
    let role = if string("role") == "user" {
        "user"
    } else {
        "assistant"
    };
    Ok(Some((role.into(), text)))
}
