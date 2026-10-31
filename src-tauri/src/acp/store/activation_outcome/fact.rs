use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn append_activation_fact(
    transaction: &rusqlite::Transaction<'_>,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    epoch: RuntimeEpoch,
    kind: &str,
    model_context: Option<crate::acp::context_handoff::budget::ModelContextLimits>,
    projection: ProjectionRevision,
    now: i64,
) -> Result<(), StoreError> {
    let payload = serde_json::to_string(&serde_json::json!({
        "kind": kind,
        "entityId": session.as_str(),
        "state": "committed",
        "runtimeEpoch": epoch.get(),
        "modelContext": model_context,
        "modelId": transaction.query_row("SELECT model_id FROM agent_sessions WHERE id=?1", [session.as_str()], |row| row.get::<_, Option<String>>(0))?
    }))
    .map_err(|_| StoreError::Corrupt("activation fact"))?;
    append_fact_payload(
        transaction,
        project.as_str(),
        chat.as_str(),
        session.as_str(),
        None,
        kind,
        &payload,
        projection,
        now,
    )
}
