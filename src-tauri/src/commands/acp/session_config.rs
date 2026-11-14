use super::*;
use crate::acp::bindings::access_policy::governs_session_modes;
use sage_acp_protocol::{
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
    let live = state.acp.live_session_config(&owner);
    let governed = governs_session_modes(state.acp.binding_registry(), binding_id.as_str());
    refuse_governed_mode_change(governed, live.as_ref(), &mutation)?;
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
/// A binding that cannot be routed (`governed` is None) is treated as
/// governed, so a routing failure never opens a direct mode change.
pub(super) fn refuse_governed_mode_change(
    governed: Option<bool>,
    live: Option<&sage_acp_protocol::session_config::SessionConfigOverlay>,
    mutation: &ConfigMutation,
) -> Result<(), AcpIpcError> {
    let changes_mode = match live {
        Some(overlay) => overlay.config.changes_mode(mutation),
        None => sage_acp_protocol::session_config::SessionConfig::may_change_mode(mutation),
    };
    if governed.unwrap_or(true) && changes_mode {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message("Change this chat's permission mode from its access menu."),
        });
    }
    Ok(())
}

/// A mode or option value for a session with no live runtime.
#[derive(Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub(crate) enum QueuedChoice {
    Mode {
        #[serde(rename = "modeId")]
        mode_id: ConfigId,
    },
    Option {
        #[serde(rename = "configId")]
        config_id: ConfigId,
        value: ConfigValue,
    },
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct QueueSessionConfigChoiceRequest {
    pub project_id: SageProjectId,
    pub project_generation: ProjectGeneration,
    pub chat_id: SageChatId,
    pub expected_projection_revision: ProjectionRevision,
    pub session_id: SageAgentSessionId,
    pub choice: QueuedChoice,
}
/// Stores a choice made while the chat's session has no live runtime. The
/// composer shows it as restored, and the restore applies it before the
/// next prompt of that session, or of the one a send starts from it, as it
/// does every stored choice.
#[tauri::command]
pub(crate) async fn queue_acp_session_config_choice(
    state: State<'_, AppState>,
    request: QueueSessionConfigChoiceRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        &state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_base_intent(
        &request.project_id,
        request.project_generation,
        &request.chat_id,
        request.expected_projection_revision,
        &snapshot,
    )
    .map_err(intent_error)?;
    // The choice is for the session the chat's next send runs: its head, or
    // with none the session it ran last, which a started session goes on from.
    let target = match snapshot.session_id.clone() {
        Some(head) => Some(head),
        None => crate::acp::store::ended_head(
            &state.database.connection(),
            request.project_id.as_str(),
            request.chat_id.as_str(),
        )
        .map_err(store_error)?
        .map(|ended| ended.session_id),
    };
    if target.as_ref() != Some(&request.session_id) {
        return Err(intent_error(IntentError::ForeignSession));
    }
    // A live runtime that has not reported its config yet takes the choice
    // once it has, as a parked session does on resume.
    let runtime =
        snapshot
            .runtime_epoch
            .map(|runtime_epoch| sage_acp_protocol::scheduler::RuntimeOwner {
                project_id: request.project_id.clone(),
                chat_id: request.chat_id.clone(),
                session_id: request.session_id.clone(),
                runtime_epoch,
            });
    let choice = match request.choice {
        QueuedChoice::Mode { mode_id } => ConfigMutation::Mode(mode_id),
        QueuedChoice::Option { config_id, value } => ConfigMutation::Option { config_id, value },
    };
    // A queued choice reaches the harness on resume, so a governed binding's
    // mode stays on the access path here too.
    let binding_id =
        session_routes::durable_binding_id(&state, &request.session_id).map_err(store_error)?;
    let governed = governs_session_modes(state.acp.binding_registry(), binding_id.as_str());
    refuse_governed_mode_change(governed, None, &choice)?;
    state
        .acp
        .queue_session_config_choice(&request.session_id, runtime.as_ref(), &choice)
        .map_err(|error| match error {
            crate::acp::service::QueueError::NotOffered | crate::acp::service::QueueError::Live => {
                AcpIpcError::Conflict {
                    resource: crate::acp::ipc::AcpConflictResource::ConfigRevision,
                    message: safe_message("The session configuration changed; reload and retry."),
                }
            }
            crate::acp::service::QueueError::Foreign => intent_error(IntentError::ForeignSession),
            crate::acp::service::QueueError::Store => AcpIpcError::RecoveryRequired {
                message: safe_message("The session configuration choice could not be saved."),
            },
        })?;
    mutation_response(&state, request.project_id, request.chat_id)
}

pub(super) fn map_config_error(error: crate::acp::runtime::HostError) -> AcpIpcError {
    match error {
        crate::acp::runtime::HostError::Intent => AcpIpcError::Conflict {
            resource: crate::acp::ipc::AcpConflictResource::ConfigRevision,
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
