use super::*;
use crate::acp::{
    bindings::access_policy::governs_session_modes,
    contract::*,
    session_config::{ConfigId, ConfigIntent, ConfigMutation, ConfigRevision, ConfigValue},
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SetSessionModeRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub expected_config_revision: ConfigRevision,
    pub mode_id: ConfigId,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SetSessionConfigOptionRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub client_intent_id: ClientIntentId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
    pub expected_config_revision: ConfigRevision,
    pub config_id: ConfigId,
    pub value: ConfigValue,
}
#[tauri::command]
pub(crate) async fn set_acp_session_mode(
    state: State<'_, AppState>,
    request: SetSessionModeRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let base = ActivateAcpSessionRequest {
        project_id: request.project_id,
        project_generation: request.project_generation,
        chat_id: request.chat_id,
        expected_projection_revision: request.expected_projection_revision,
        client_intent_id: request.client_intent_id,
        session_id: request.session_id,
        runtime_epoch: request.runtime_epoch,
    };
    configure(
        &state,
        base,
        request.expected_config_revision,
        ConfigMutation::Mode(request.mode_id),
    )
    .await
}
#[tauri::command]
pub(crate) async fn set_acp_session_config_option(
    state: State<'_, AppState>,
    request: SetSessionConfigOptionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let base = ActivateAcpSessionRequest {
        project_id: request.project_id,
        project_generation: request.project_generation,
        chat_id: request.chat_id,
        expected_projection_revision: request.expected_projection_revision,
        client_intent_id: request.client_intent_id,
        session_id: request.session_id,
        runtime_epoch: request.runtime_epoch,
    };
    configure(
        &state,
        base,
        request.expected_config_revision,
        ConfigMutation::Option {
            config_id: request.config_id,
            value: request.value,
        },
    )
    .await
}
pub(crate) async fn configure(
    state: &AppState,
    request: ActivateAcpSessionRequest,
    expected_config_revision: ConfigRevision,
    mutation: ConfigMutation,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    // Durable revision authority is always checked before ephemeral config authority.
    validate_base_intent(
        &request.project_id,
        request.project_generation,
        &request.chat_id,
        request.expected_projection_revision,
        &snapshot,
    )
    .map_err(intent_error)?;
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(intent_error(IntentError::ForeignSession));
    }
    if snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(intent_error(IntentError::StaleRuntimeEpoch));
    }
    let owner = RuntimeOwner {
        project_id: request.project_id.clone(),
        chat_id: request.chat_id.clone(),
        session_id: request.session_id,
        runtime_epoch: request.runtime_epoch,
    };
    let binding_id =
        session_routes::durable_binding_id(state, &owner.session_id).map_err(store_error)?;
    refuse_governed_mode_change(
        governs_session_modes(state.acp.binding_registry(), binding_id.as_str()),
        state.acp.live_session_config(&owner).as_ref(),
        &mutation,
    )?;
    state
        .acp
        .configure_session(ConfigIntent {
            owner,
            expected_config_revision,
            mutation,
        })
        .map_err(map_config_error)?;
    mutation_response(state, request.project_id, request.chat_id)
}

/// Where a binding's access is enforced through its session modes, only the
/// access path (`session_access::select_session_mode`) may change the mode:
/// a direct mode change here could select a mode, such as Claude's
/// bypassPermissions, without the chat's access choice or its confirmation.
pub(super) fn refuse_governed_mode_change(
    governed: bool,
    live: Option<&crate::acp::session_config::SessionConfigOverlay>,
    mutation: &ConfigMutation,
) -> Result<(), AcpIpcError> {
    let changes_mode = match live {
        Some(overlay) => overlay.config.changes_mode(mutation),
        None => crate::acp::session_config::SessionConfig::may_change_mode(mutation),
    };
    if governed && changes_mode {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message("Change this chat's permission mode from its access menu."),
        });
    }
    Ok(())
}

pub(super) fn map_config_error(error: crate::acp::runtime::HostError) -> AcpIpcError {
    match error {
        crate::acp::runtime::HostError::Intent => AcpIpcError::Conflict {
            resource: crate::acp::intent::AcpConflictResource::ConfigRevision,
            message: safe_message("The live session configuration changed; reload and retry."),
        },
        crate::acp::runtime::HostError::ConfigMutationRejected => AcpIpcError::Rejected {
            message: safe_message("The harness rejected this session configuration change."),
        },
        crate::acp::runtime::HostError::SignInRequired => AcpIpcError::SignInRequired {
            message: safe_message("The harness requires sign-in before it can continue."),
        },
        _ => AcpIpcError::RecoveryRequired {
            message: safe_message(
                "Session configuration could not be confirmed; reload the authoritative session.",
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlated_harness_refusal_maps_to_bounded_rejected_ipc_failure() {
        assert!(matches!(
            map_config_error(crate::acp::runtime::HostError::ConfigMutationRejected),
            AcpIpcError::Rejected { message }
                if message.as_str()
                    == "The harness rejected this session configuration change."
        ));
    }

    #[test]
    fn transport_failure_still_requires_authoritative_recovery() {
        assert!(matches!(
            map_config_error(crate::acp::runtime::HostError::DeliveryUnknown),
            AcpIpcError::RecoveryRequired { .. }
        ));
    }

    #[test]
    fn sign_in_refusal_maps_to_the_stable_sign_in_ipc_failure() {
        assert!(matches!(
            map_config_error(crate::acp::runtime::HostError::SignInRequired),
            AcpIpcError::SignInRequired { message }
                if message.as_str() == "The harness requires sign-in before it can continue."
        ));
    }
}
