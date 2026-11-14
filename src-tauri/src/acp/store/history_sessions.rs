//! The sessions of one chat, as the chat page projects them.

use rusqlite::params;

use crate::acp::{
    contract::{
        EntityRevision, ProjectionRevision, RuntimeEpoch, SageAgentSessionId, SageChatId,
        SageProjectId,
    },
    intent::{BindingId, CapabilitySnapshotV1, ModelId},
    lifecycle::Residency,
    projection::SessionProjection,
};

use super::StoreError;

pub(super) fn load_sessions(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
    chat: &SageChatId,
) -> Result<Vec<SessionProjection>, StoreError> {
    let mut statement=connection.prepare("SELECT s.id,s.ordinal,s.binding_id,s.model_id,s.state,s.residency,(SELECT MAX(e.runtime_epoch) FROM session_runtime_epochs e WHERE e.session_id=s.id AND e.state IN ('starting','running','terminating')),(SELECT MAX(e.runtime_epoch) FROM session_runtime_epochs e WHERE e.session_id=s.id),s.capabilities_json,s.revision,s.projection_revision,s.legacy FROM agent_sessions s WHERE s.project_id=?1 AND s.chat_id=?2 ORDER BY s.ordinal,id LIMIT 33")?;
    let rows = statement.query_map(params![project.as_str(), chat.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
        ))
    })?;
    let mut result = Vec::with_capacity(32);
    for row in rows {
        if result.len() == 32 {
            return Err(StoreError::Corrupt("session page limit+1"));
        }
        let row = row?;
        let state = super::state::parse_session_state(&row.4)?;
        let residency = match row.5.as_str() {
            "hot" => Residency::Hot,
            "cold" => Residency::Cold,
            _ => return Err(StoreError::Corrupt("residency")),
        };
        let runtime_epoch = row
            .6
            .map(|value| {
                RuntimeEpoch::new(
                    u64::try_from(value).map_err(|_| StoreError::Corrupt("runtime epoch"))?,
                )
                .map_err(|_| StoreError::Corrupt("runtime epoch"))
            })
            .transpose()?;
        let next_runtime_epoch = if runtime_epoch.is_none()
            && residency == Residency::Cold
            && matches!(
                state,
                crate::acp::lifecycle::SessionState::Created
                    | crate::acp::lifecycle::SessionState::Ready
            ) {
            let latest = row.7.unwrap_or(0);
            let next = latest.checked_add(1).ok_or(StoreError::CounterExhausted)?;
            Some(
                RuntimeEpoch::new(
                    u64::try_from(next).map_err(|_| StoreError::Corrupt("runtime epoch"))?,
                )
                .map_err(|_| StoreError::Corrupt("runtime epoch"))?,
            )
        } else {
            None
        };
        let binding_id =
            BindingId::new("bindingId", row.2).map_err(|_| StoreError::Corrupt("binding id"))?;
        let capabilities = serde_json::from_str::<CapabilitySnapshotV1>(&row.8)
            .map_err(|_| StoreError::Corrupt("capabilities"))?;
        result.push(SessionProjection {
            id: SageAgentSessionId::parse(row.0).map_err(|_| StoreError::Corrupt("session id"))?,
            ordinal: u64::try_from(row.1).map_err(|_| StoreError::Corrupt("session ordinal"))?,
            binding_id: binding_id.clone(),
            model_id: row
                .3
                .map(|v| ModelId::new("modelId", v))
                .transpose()
                .map_err(|_| StoreError::Corrupt("model id"))?,
            state,
            residency,
            runtime_epoch,
            next_runtime_epoch,
            permission_requests: permission_requests_supported(&binding_id, &capabilities),
            capabilities,
            revision: EntityRevision::new(row.9 as u64)
                .map_err(|_| StoreError::Corrupt("session revision"))?,
            projection_revision: ProjectionRevision::new(row.10 as u64)
                .map_err(|_| StoreError::Corrupt("projection revision"))?,
            legacy: match row.11 {
                0 => false,
                1 => true,
                _ => return Err(StoreError::Corrupt("legacy flag")),
            },
        });
    }
    Ok(result)
}

fn permission_requests_supported(
    binding_id: &BindingId,
    capabilities: &CapabilitySnapshotV1,
) -> bool {
    match binding_id.as_str() {
        "codex-app-server" => {
            crate::acp_protocol::codex_protocol::speaks_pinned_protocol(
                capabilities.protocol_version.as_str(),
            ) && capabilities.active_turn_steer
                && crate::acp::bindings::access_policy::CODEX_ASK_NATIVE_APPROVAL_PROVEN
        }
        "claude-agent-acp" | "claude-code-sdk" | "cursor-agent-acp" | "opencode-acp"
        | "gemini-cli-acp" => true,
        _ => false,
    }
}

#[cfg(test)]
mod permission_request_tests {
    use super::*;

    fn capabilities(protocol_version: &str) -> CapabilitySnapshotV1 {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "protocolVersion": protocol_version,
            "newSession": true,
            "resumeSession": true,
            "loadSession": true,
            "closeSession": true,
            "prompt": true,
            "cancel": true,
            "nativeSessionFork": false,
            "activeTurnSteer": false,
            "childConversationProjection": false,
            "childTranscriptRead": false,
            "collaborationEventProjection": false,
            "multiRootExtensionSchemaId": null,
            "modelCount": 0,
            "configCount": 0,
        }))
        .expect("capabilities")
    }

    #[test]
    fn codex_ask_projection_requires_native_approval_evidence() {
        let codex = BindingId::new("bindingId", "codex-app-server").expect("Codex binding");
        assert!(!permission_requests_supported(
            &codex,
            &capabilities("codex-app-server-0.145.0-7b0ab167")
        ));
        let mut managed =
            capabilities(crate::acp_protocol::codex_protocol::PINNED_PROTOCOL_IDENTITY);
        managed.active_turn_steer = true;
        assert!(!permission_requests_supported(&codex, &managed));
        let mut observed =
            capabilities(crate::acp_protocol::codex_protocol::OBSERVED_PROTOCOL_IDENTITY);
        assert!(!permission_requests_supported(&codex, &observed));
        observed.active_turn_steer = true;
        let advertised = crate::acp::bindings::access_policy::access_modes(
            "codex-app-server",
            crate::acp_protocol::codex_protocol::PINNED_CODEX_VERSION,
            true,
        )
        .iter()
        .any(|mode| mode.id == crate::acp_protocol::access_mode::SessionAccessMode::Ask);
        assert_eq!(permission_requests_supported(&codex, &observed), advertised);

        let claude = BindingId::new("bindingId", "claude-agent-acp").expect("Claude binding");
        assert!(permission_requests_supported(&claude, &capabilities("1")));

        let unknown = BindingId::new("bindingId", "fixture").expect("fixture binding");
        assert!(!permission_requests_supported(
            &unknown,
            &capabilities("fixture-1")
        ));
    }
}
