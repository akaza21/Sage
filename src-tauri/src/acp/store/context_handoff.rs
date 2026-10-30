// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
use super::{
    PrepareTurnCommand, StoreError,
    facts::{check_add, hex_digest, projection_from_i64},
    sessions::ensure_one,
};
use crate::acp::{
    context_handoff::{
        HandoffPlan, HandoffState, HandoffSummary,
        budget::{BudgetInput, handoff_byte_budget},
        history, render,
    },
    contract::ProjectionRevision,
    intent::Prompt,
    limits::LIMITS,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub(crate) fn charge(
    transaction: &Transaction<'_>,
    session: &str,
    bytes: i64,
) -> Result<(), StoreError> {
    let (project, current): (String, i64) = transaction.query_row(
        "SELECT project_id,payload_bytes FROM acp_session_usage WHERE session_id=?1",
        [session],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let global: i64 = transaction.query_row(
        "SELECT shared_payload_bytes FROM acp_store_usage WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    check_add(
        current,
        bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "sessionHandoffPayload",
    )?;
    check_add(
        global,
        bytes,
        LIMITS.durable.shared_payload_bytes_global,
        "globalHandoffPayload",
    )?;
    ensure_one(transaction.execute("UPDATE acp_session_usage SET payload_bytes=payload_bytes+?2,revision=revision+1 WHERE session_id=?1", params![session,bytes])?, "session usage")?;
    ensure_one(transaction.execute("UPDATE acp_project_usage SET payload_bytes=payload_bytes+?2,revision=revision+1 WHERE project_id=?1", params![project,bytes])?, "project usage")?;
    ensure_one(transaction.execute("UPDATE acp_store_usage SET shared_payload_bytes=shared_payload_bytes+?1,revision=revision+1 WHERE singleton=1", [bytes])?, "global usage")
}

pub(crate) fn latest_delivery_ambiguous(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<bool, StoreError> {
    Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM acp_context_handoffs h JOIN agent_sessions s ON s.id=h.target_session
        WHERE h.project_id=?1 AND h.chat_id=?2 AND h.state IN ('pending','unknown')
        AND s.ordinal=(SELECT MAX(ordinal) FROM agent_sessions WHERE project_id=?1 AND chat_id=?2))",
        params![project,chat], |row|row.get(0))?)
}

pub(crate) fn freeze_prompt(
    connection: &mut Connection,
    command: &mut PrepareTurnCommand,
) -> Result<(), StoreError> {
    let transaction = connection.transaction()?;
    let row: Option<(i64, Option<String>, String, Option<String>)> = transaction.query_row(
        "SELECT source_watermark,plan_json,state,digest FROM acp_context_handoffs WHERE project_id=?1 AND chat_id=?2 AND target_session=?3",
        params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str()], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?;
    let Some((watermark, payload, state, digest)) = row else {
        return Ok(());
    };
    match state.as_str() {
        "inline" => return Ok(()),
        "unknown" | "pending" => {
            return Err(StoreError::Conflict {
                resource: "handoffDeliveryUnknown",
            });
        }
        "prepared" => {}
        _ => return Err(StoreError::Corrupt("handoff delivery state")),
    }
    super::sessions::validate_chat_revision(
        &transaction,
        &command.project_id,
        &command.chat_id,
        command.expected_projection_revision,
    )?;
    let exact:bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM chat_session_heads h JOIN session_runtime_epochs e ON e.session_id=h.session_id WHERE h.project_id=?1 AND h.chat_id=?2 AND h.session_id=?3 AND e.runtime_epoch=?4 AND e.state IN ('starting','running'))",
        params![command.project_id.as_str(),command.chat_id.as_str(),command.session_id.as_str(),command.runtime_epoch.get() as i64],|row|row.get(0))?;
    if !exact {
        return Err(StoreError::Conflict {
            resource: "handoffOwner",
        });
    }
    let byte_budget = prompt_byte_budget(command)?;
    let plan = if let Some(payload) = payload {
        if digest.as_deref() != Some(hex_digest(payload.as_bytes()).as_str()) {
            return Err(StoreError::Corrupt("handoff plan digest"));
        }
        decode_plan(&payload)?
    } else {
        let history = history::load(
            &transaction,
            command.project_id.as_str(),
            command.chat_id.as_str(),
            watermark,
        )?;
        let plan = render::select_history(&history, byte_budget)?;
        let payload =
            serde_json::to_string(&plan).map_err(|_| StoreError::Corrupt("handoff plan"))?;
        if payload.len() > 262_144 {
            return Err(StoreError::PayloadTooLarge);
        }
        charge(
            &transaction,
            command.session_id.as_str(),
            payload.len() as i64,
        )?;
        transaction.execute("UPDATE acp_context_handoffs SET plan_json=?2,digest=?3,updated_at=?4 WHERE target_session=?1 AND plan_json IS NULL",
            params![command.session_id.as_str(),payload,hex_digest(payload.as_bytes()),command.now_millis])?;
        plan
    };
    if plan.estimated_cost_bytes as u64 > byte_budget {
        return Err(StoreError::HandoffBudgetExceeded);
    }
    let combined = render::combine_prompt(
        &plan.rendered_text,
        command.prompt.as_str(),
        LIMITS.wire.frame_bytes as usize,
    )?;
    command.prompt = Prompt::new("prompt", combined)
        .map_err(|_| StoreError::HandoffBudgetExceeded)?
        .with_history(&command.prompt);
    transaction.commit().map_err(|_| StoreError::CommitUnknown)
}

fn prompt_byte_budget(command: &PrepareTurnCommand) -> Result<u64, StoreError> {
    let user = command.prompt.user_payload();
    let mut budget = BudgetInput::fresh(user["text"].as_str().unwrap_or_default());
    if let Some(attachments) = user
        .get("attachedContext")
        .and_then(serde_json::Value::as_array)
    {
        for attachment in attachments {
            if attachment["kind"] == "image" {
                budget.images += 1;
            } else {
                budget.other_attachments += 1;
            }
        }
    }
    budget.configured_byte_cap = std::env::var("SAGE_CONTEXT_HANDOFF_BYTE_CAP")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(16_000);
    let prompt_cost = serde_json::to_vec(command.prompt.as_str())
        .map_err(|_| StoreError::HandoffBudgetExceeded)?
        .len();
    let transport_budget = (LIMITS.wire.prompt_bytes as usize)
        .saturating_sub(prompt_cost)
        .saturating_sub(512);
    Ok(handoff_byte_budget(&budget).min(transport_budget as u64))
}

fn decode_plan(payload: &str) -> Result<HandoffPlan, StoreError> {
    if payload.len() > 262_144 {
        return Err(StoreError::Corrupt("handoff plan bytes"));
    }
    let plan: HandoffPlan =
        serde_json::from_str(payload).map_err(|_| StoreError::Corrupt("handoff plan"))?;
    if plan.selected_ids.len() > 512
        || plan.rendered_text.len() > 64_000
        || plan.selected_bytes != plan.rendered_text.len()
        || plan.estimated_cost_bytes < plan.selected_bytes
        || plan.estimated_cost_bytes > 64_000
        || plan.source_watermark < 0
        || plan.budget_version != 1
    {
        return Err(StoreError::Corrupt("handoff plan bounds"));
    }
    Ok(plan)
}

pub(crate) fn summaries(
    connection: &Connection,
    project: &str,
    chat: &str,
) -> Result<Vec<HandoffSummary>, StoreError> {
    let mut statement = connection.prepare("SELECT h.id,h.target_session,h.turn_id,h.state,h.projection_revision,h.plan_json FROM acp_context_handoffs h
        JOIN agent_sessions s ON s.id=h.target_session WHERE h.project_id=?1 AND h.chat_id=?2 ORDER BY s.ordinal DESC LIMIT 32")?;
    let mut result = Vec::new();
    let rows = statement.query_map(params![project, chat], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    for row in rows {
        let (id, target_session, turn_id, state, revision, payload) = row?;
        let plan = payload.as_deref().map(decode_plan).transpose()?;
        let state = match state.as_str() {
            "prepared" => HandoffState::Prepared,
            "pending" => HandoffState::Pending,
            "inline" => HandoffState::Inline,
            "unknown" => HandoffState::Unknown,
            _ => return Err(StoreError::Corrupt("handoff state")),
        };
        result.push(HandoffSummary {
            id,
            target_session,
            turn_id,
            state,
            revision: projection_from_i64(revision)?,
            carried_items: plan.as_ref().map_or(0, |plan| plan.selected_ids.len()),
            carried_bytes: plan.as_ref().map_or(0, |plan| plan.selected_bytes),
            omissions: plan.map_or_else(Default::default, |plan| plan.omissions),
        });
    }
    Ok(result)
}

pub(crate) fn bind_turn(
    transaction: &Transaction<'_>,
    command: &PrepareTurnCommand,
    turn: &str,
    projection: ProjectionRevision,
) -> Result<(), StoreError> {
    transaction.execute("UPDATE acp_context_handoffs SET turn_id=?2,runtime_epoch=?3,native_id=(SELECT external_session_id FROM agent_sessions WHERE id=?1),projection_revision=?4,updated_at=?5
        WHERE target_session=?1 AND state='prepared' AND plan_json IS NOT NULL", params![command.session_id.as_str(),turn,command.runtime_epoch.get() as i64,projection.get() as i64,command.now_millis])?;
    Ok(())
}
