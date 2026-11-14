use rusqlite::{Row, Transaction, params};
use sage_core::error::{AppError, AppResult};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::acp::{
    contract::{BoundedVec, SafeText},
    declared_test::CommandActivity,
    intent::{
        COMMAND_RESOURCE_LIMIT, NormalizedPayload, SafeResourceLocation, ToolCategory,
        ToolExecution,
    },
};

const SUSPENDED_TRIGGERS: [&str; 6] = [
    "deny_item_invalid_transition",
    "deny_terminal_item_mutation",
    "deny_tool_invalid_transition",
    "deny_item_delete",
    "deny_activity_identity_mutation",
    "deny_terminal_activity_mutation",
];

const PAYLOAD_BYTES_LIMIT: usize = 262_144;

const PAGE_ROWS: i64 = 128;

const PAGE_BYTES: i64 = 4_194_304;

const TERMINAL_LIFECYCLES: [&str; 4] = ["completed", "failed", "cancelled", "unsupported"];

struct LegacyUpdate {
    id: String,
    project_id: String,
    session_id: String,
    payload_json: String,
    lifecycle: String,
    tool_state: Option<String>,
    terminal_at: Option<i64>,
    updated_at: i64,
    call: Option<SiblingCall>,
}

struct SiblingCall {
    id: String,
    payload_json: String,
    payload_bytes: i64,
    lifecycle: String,
    tool_state: Option<String>,
    terminal_at: Option<i64>,
    updated_at: i64,
}

struct FoldedStatus {
    lifecycle: String,
    tool_state: Option<String>,
    terminal_at: Option<i64>,
    updated_at: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum LegacyToolKind {
    ToolCall,
    ToolUpdateResult,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyToolEnvelope {
    kind: LegacyToolKind,
    payload: LegacyToolPayload,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyToolPayload {
    category: ToolCategory,
    label: SafeText<512>,
    #[serde(default)]
    input_summary: Option<SafeText<65536>>,
    #[serde(default)]
    output_summary: Option<SafeText<65536>>,
    #[serde(default)]
    locations: BoundedVec<SafeResourceLocation, COMMAND_RESOURCE_LIMIT>,
    #[serde(default)]
    command_activity: CommandActivity,
    #[serde(default)]
    execution: ToolExecution,
}

pub(super) fn apply(transaction: &Transaction<'_>) -> AppResult<()> {
    let restored = suspend_triggers(transaction)?;
    let mut cursor = String::new();
    loop {
        let page = load_page(transaction, &cursor)?;
        let Some(last) = page.last() else {
            break;
        };
        cursor = last.id.clone();
        for update in page {
            fold(transaction, update)?;
        }
    }
    for statement in restored {
        transaction.execute_batch(&statement)?;
    }
    Ok(())
}

fn suspend_triggers(transaction: &Transaction<'_>) -> AppResult<Vec<String>> {
    let mut restored = Vec::new();
    for name in SUSPENDED_TRIGGERS {
        let definition: Option<String> = transaction
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
                [name],
                |row| row.get(0),
            )
            .ok();
        if let Some(definition) = definition {
            transaction.execute_batch(&format!("DROP TRIGGER {name}"))?;
            restored.push(definition);
        }
    }
    Ok(restored)
}

const PAGE_SQL: &str = "SELECT u.id,u.project_id,u.session_id,
                u.payload_json,u.lifecycle,u.tool_state,u.terminal_at,u.updated_at,
                c.id,c.payload_json,c.payload_bytes,c.lifecycle,c.tool_state,c.terminal_at,c.updated_at
         FROM items u
         LEFT JOIN items c
           ON c.kind='toolCall' AND c.session_id=u.session_id
              AND c.runtime_epoch=u.runtime_epoch
              AND c.external_item_id=u.external_item_id
         WHERE u.kind='toolUpdateResult' AND u.id>?1
         ORDER BY u.id
         LIMIT ?2";

fn load_page(transaction: &Transaction<'_>, cursor: &str) -> AppResult<Vec<LegacyUpdate>> {
    let mut statement = transaction.prepare(PAGE_SQL)?;
    let mut rows = statement.query(params![cursor, PAGE_ROWS])?;
    let mut page = Vec::new();
    let mut bytes = 0_i64;
    while let Some(row) = rows.next()? {
        let update = legacy_update(row)?;
        let cost = update.payload_json.len() as i64
            + update.call.as_ref().map_or(0, |call| call.payload_bytes);
        if !page.is_empty() && bytes + cost > PAGE_BYTES {
            break;
        }
        bytes += cost;
        page.push(update);
    }
    Ok(page)
}

#[cfg(test)]
pub(super) const PAGE_BYTES_BOUND: i64 = PAGE_BYTES;

#[cfg(test)]
pub(super) fn page_for_test(
    transaction: &Transaction<'_>,
    cursor: &str,
) -> AppResult<(Vec<String>, i64)> {
    let page = load_page(transaction, cursor)?;
    let bytes = page
        .iter()
        .map(|update| {
            update.payload_json.len() as i64
                + update.call.as_ref().map_or(0, |call| call.payload_bytes)
        })
        .sum();
    Ok((page.into_iter().map(|update| update.id).collect(), bytes))
}

#[cfg(test)]
pub(super) fn page_plan_for_test(transaction: &Transaction<'_>) -> AppResult<String> {
    let mut statement = transaction.prepare(&format!("EXPLAIN QUERY PLAN {PAGE_SQL}"))?;
    let rows = statement.query_map(params!["", PAGE_ROWS], |row| row.get::<_, String>(3))?;
    let mut plan = String::new();
    for row in rows {
        plan.push_str(&row?);
        plan.push('\n');
    }
    Ok(plan)
}

fn legacy_update(row: &Row<'_>) -> rusqlite::Result<LegacyUpdate> {
    Ok(LegacyUpdate {
        id: row.get(0)?,
        project_id: row.get(1)?,
        session_id: row.get(2)?,
        payload_json: row.get(3)?,
        lifecycle: row.get(4)?,
        tool_state: row.get(5)?,
        terminal_at: row.get(6)?,
        updated_at: row.get(7)?,
        call: row
            .get::<_, Option<String>>(8)?
            .map(|id| {
                Ok::<SiblingCall, rusqlite::Error>(SiblingCall {
                    id,
                    payload_json: row.get(9)?,
                    payload_bytes: row.get(10)?,
                    lifecycle: row.get(11)?,
                    tool_state: row.get(12)?,
                    terminal_at: row.get(13)?,
                    updated_at: row.get(14)?,
                })
            })
            .transpose()?,
    })
}

fn fold(transaction: &Transaction<'_>, update: LegacyUpdate) -> AppResult<()> {
    match update.call {
        Some(ref call) => {
            let preserve_call_presentation =
                is_terminal(&call.lifecycle) && !is_terminal(&update.lifecycle);
            let merged = merged_payload(
                &update.payload_json,
                Some(&call.payload_json),
                preserve_call_presentation,
            )?;
            let status = folded_status(&update, call);
            transaction.execute(
                "UPDATE items SET payload_json=?1,payload_bytes=?2,payload_digest=?3,
                        lifecycle=?4,tool_state=?5,terminal_at=?6,updated_at=?7,revision=revision+1
                 WHERE id=?8",
                params![
                    merged,
                    merged.len() as i64,
                    digest(&merged),
                    status.lifecycle,
                    status.tool_state,
                    status.terminal_at,
                    status.updated_at,
                    call.id,
                ],
            )?;
            release(
                transaction,
                &update.project_id,
                &update.session_id,
                merged.len() as i64 - call.payload_bytes,
            )?;
            transaction.execute(
                "UPDATE session_activities SET parent_item_id=?1 WHERE parent_item_id=?2",
                params![call.id, update.id],
            )?;
            transaction.execute("DELETE FROM items WHERE id=?1", [&update.id])?;
        }
        None => {
            let merged = merged_payload(&update.payload_json, None, false)?;
            transaction.execute(
                "UPDATE items SET kind='toolCall',payload_json=?1,payload_bytes=?2,
                        payload_digest=?3,revision=revision+1
                 WHERE id=?4",
                params![merged, merged.len() as i64, digest(&merged), update.id],
            )?;
            release(
                transaction,
                &update.project_id,
                &update.session_id,
                merged.len() as i64 - update.payload_json.len() as i64,
            )?;
        }
    }
    Ok(())
}

fn folded_status(update: &LegacyUpdate, call: &SiblingCall) -> FoldedStatus {
    let updated_at = update.updated_at.max(call.updated_at);
    if is_terminal(&call.lifecycle) && !is_terminal(&update.lifecycle) {
        return FoldedStatus {
            lifecycle: call.lifecycle.clone(),
            tool_state: call.tool_state.clone(),
            terminal_at: call.terminal_at,
            updated_at,
        };
    }
    FoldedStatus {
        lifecycle: update.lifecycle.clone(),
        tool_state: update.tool_state.clone(),
        terminal_at: update.terminal_at,
        updated_at,
    }
}

fn is_terminal(lifecycle: &str) -> bool {
    TERMINAL_LIFECYCLES.contains(&lifecycle)
}

fn merged_payload(
    update: &str,
    call: Option<&str>,
    preserve_call_presentation: bool,
) -> AppResult<String> {
    let update = decoded(update, LegacyToolKind::ToolUpdateResult)?;
    let call = call
        .map(|call| decoded(call, LegacyToolKind::ToolCall))
        .transpose()?;
    let merged = serde_json::to_string(&update.merged(call, preserve_call_presentation)).map_err(
        |error| {
            AppError::InvalidRequest(format!(
                "folded tool call payload failed to encode: {error}"
            ))
        },
    )?;
    if merged.len() > PAYLOAD_BYTES_LIMIT {
        return Err(AppError::ResourceLimit(format!(
            "folded tool call payload of {} bytes exceeds the {PAYLOAD_BYTES_LIMIT} byte item bound",
            merged.len()
        )));
    }
    Ok(merged)
}

fn decoded(payload: &str, kind: LegacyToolKind) -> AppResult<LegacyToolPayload> {
    let envelope: LegacyToolEnvelope = serde_json::from_str(payload).map_err(|error| {
        AppError::InvalidRequest(format!(
            "legacy tool payload is not a normalized tool envelope: {error}"
        ))
    })?;
    if envelope.kind != kind {
        return Err(AppError::InvalidRequest(
            "legacy tool payload kind disagrees with its stored item kind".to_owned(),
        ));
    }
    Ok(envelope.payload)
}

impl LegacyToolPayload {
    fn merged(mut self, call: Option<Self>, preserve_call_presentation: bool) -> NormalizedPayload {
        if let Some(call) = call {
            if preserve_call_presentation {
                self.category = call.category;
                self.label = call.label;
                self.locations = call.locations;
                self.command_activity = call.command_activity;
                self.execution = call.execution;
            } else {
                if self.label.as_str().is_empty() {
                    self.label = call.label;
                }
                if self.locations.as_slice().is_empty() {
                    self.locations = call.locations;
                }
                if self.command_activity == CommandActivity::default() {
                    self.command_activity = call.command_activity;
                }
                self.execution = merged_execution(self.execution, call.execution);
            }
            self.input_summary = self.input_summary.or(call.input_summary);
            self.output_summary = self.output_summary.or(call.output_summary);
        }
        NormalizedPayload::ToolCall {
            category: self.category,
            label: self.label,
            input_summary: self.input_summary,
            output_summary: self.output_summary,
            locations: self.locations,
            command_activity: self.command_activity,
            execution: self.execution,
        }
    }
}

fn merged_execution(update: ToolExecution, call: ToolExecution) -> ToolExecution {
    ToolExecution {
        cwd: update.cwd.or(call.cwd),
        exit_code: update.exit_code.or(call.exit_code),
        output_tail: update.output_tail.or(call.output_tail),
        output_truncated: update.output_truncated || call.output_truncated,
        task_lifecycle: update.task_lifecycle.or(call.task_lifecycle),
        mcp_server: update.mcp_server.or(call.mcp_server),
        // A running call's status line never outlives its result.
        progress: update.progress,
    }
}

fn digest(payload: &str) -> String {
    format!("{:x}", Sha256::digest(payload.as_bytes()))
}

fn release(
    transaction: &Transaction<'_>,
    project_id: &str,
    session_id: &str,
    delta: i64,
) -> AppResult<()> {
    if delta == 0 {
        return Ok(());
    }
    transaction.execute(
        "UPDATE acp_session_usage
            SET item_bytes=item_bytes+?1,payload_bytes=payload_bytes+?1,revision=revision+1
          WHERE session_id=?2",
        params![delta, session_id],
    )?;
    transaction.execute(
        "UPDATE acp_project_usage
            SET payload_bytes=payload_bytes+?1,revision=revision+1
          WHERE project_id=?2",
        params![delta, project_id],
    )?;
    transaction.execute(
        "UPDATE acp_store_usage
            SET item_bytes=item_bytes+?1,shared_payload_bytes=shared_payload_bytes+?1,
                revision=revision+1
          WHERE singleton=1",
        [delta],
    )?;
    Ok(())
}
