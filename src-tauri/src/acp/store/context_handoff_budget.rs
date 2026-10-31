use super::{PrepareTurnCommand, StoreError};
use crate::acp::context_handoff::budget::{BudgetInput, ModelContextLimits};
use rusqlite::{Connection, OptionalExtension, params};

/// Activation facts carry only the admitted, selected model's capacities.
/// Never borrow source-session telemetry for a fresh native conversation.
pub(super) fn populate(
    connection: &Connection,
    command: &PrepareTurnCommand,
    live_model: Option<&crate::acp::session_config::ConfirmedModel>,
    budget: &mut BudgetInput<'_>,
) -> Result<(), StoreError> {
    let payload: Option<String> = connection
        .query_row(
            "SELECT f.payload_json FROM session_facts f JOIN agent_sessions s ON s.id=f.session_id
         WHERE f.project_id=?1 AND f.chat_id=?2 AND f.session_id=?3 AND f.kind='activationReady'
         AND json_extract(f.payload_json,'$.runtimeEpoch')=?4
         AND json_extract(f.payload_json,'$.modelId') IS s.model_id
         ORDER BY f.sequence DESC LIMIT 1",
            params![
                command.project_id.as_str(),
                command.chat_id.as_str(),
                command.session_id.as_str(),
                command.runtime_epoch.get() as i64
            ],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(payload) = payload {
        let value: serde_json::Value = serde_json::from_str(&payload)
            .map_err(|_| StoreError::Corrupt("activation model context"))?;
        if let Some(value) = value.get("modelContext").filter(|value| !value.is_null()) {
            let limits: ModelContextLimits = serde_json::from_value(value.clone())
                .map_err(|_| StoreError::Corrupt("activation model context limits"))?;
            budget.model_window_bytes = limits.model_window_bytes.map(u64::from);
            budget.reported_max_bytes = limits.reported_max_bytes.map(u64::from);
            budget.auto_compact_bytes = limits.auto_compact_bytes.map(u64::from);
        }
    }
    if let Some(model) = live_model {
        // The exact owner's confirmed native model is authoritative even if
        // the harness resolved a requested alias to a different model ID.
        budget.model_window_bytes = model
            .context_limits
            .model_window_bytes
            .map(u64::from)
            .or(budget.model_window_bytes);
        budget.reported_max_bytes = model
            .context_limits
            .reported_max_bytes
            .map(u64::from)
            .or(budget.reported_max_bytes);
        budget.auto_compact_bytes = model
            .context_limits
            .auto_compact_bytes
            .map(u64::from)
            .or(budget.auto_compact_bytes);
    }
    let usage: Option<(u64, Option<u64>)> = connection.query_row(
        "SELECT u.input_tokens,u.context_window FROM acp_turn_usage_snapshots u
         JOIN turns t ON t.id=u.turn_id AND t.session_id=u.session_id AND t.runtime_epoch=u.runtime_epoch
         JOIN agent_sessions s ON s.id=u.session_id
         JOIN session_runtime_epochs e ON e.session_id=s.id AND e.runtime_epoch=u.runtime_epoch
         WHERE u.project_id=?1 AND u.chat_id=?2 AND u.session_id=?3 AND u.runtime_epoch=?4
         AND s.external_session_id IS NOT NULL AND e.state IN ('starting','running')
         ORDER BY t.ordinal DESC LIMIT 1",
        params![command.project_id.as_str(), command.chat_id.as_str(), command.session_id.as_str(), command.runtime_epoch.get() as i64],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    if let Some((occupancy, maximum)) = usage {
        budget.same_native_identity = true;
        budget.native_occupancy_bytes = occupancy;
        if let Some(maximum) = maximum.filter(|maximum| *maximum > 0) {
            budget.reported_max_bytes = Some(
                budget
                    .reported_max_bytes
                    .map_or(maximum, |known| known.min(maximum)),
            );
        }
    }
    Ok(())
}
