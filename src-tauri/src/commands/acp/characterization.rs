use crate::{
    acp::{
        contract::SafeText,
        intent::{
            AcpCapacityResource, AcpConflictResource, AcpIpcError, AcpMutationResponse,
            AcpUnavailableReason, ActivateAcpSessionRequest, CancelAcpTurnRequest,
            CloseAcpSessionRequest, SendAcpTurnRequest, StartAcpSessionRequest,
        },
        limits::LIMITS,
        runtime::HostCancelIntent,
        scheduler::RuntimeOwner,
        service::{CHARACTERIZATION_BINDING_DIGEST, CharacterizationError},
        store::{
            ActivateSessionCommand, CloseSessionCommand, PrepareTurnCommand, StartSessionCommand,
        },
    },
    project_registry::ProjectRootAuthority,
    state::AppState,
};

use super::{errors::safe_message, mutation_response};

pub(super) async fn start(
    state: &AppState,
    request: StartAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .start_characterization_session(StartSessionCommand {
            project_id: request.project_id,
            chat_id: request.chat_id,
            expected_projection_revision: request.expected_projection_revision,
            client_intent_id: request.client_intent_id,
            binding_id: request.binding_id,
            binding_digest: CHARACTERIZATION_BINDING_DIGEST.to_owned(),
            harness_version: Some(
                SafeText::new("harnessVersion", "fixture-1")
                    .expect("static characterization version is bounded"),
            ),
            model_id: request.model_id,
            now_millis: crate::persistence::now_millis(),
        })
        .await
        .map_err(map_error)?;
    mutation_response(state, project_id, chat_id)
}

pub(super) async fn activate(
    state: &AppState,
    request: ActivateAcpSessionRequest,
    project_root: ProjectRootAuthority,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    let artifact_digest = state
        .acp
        .characterization_artifact_digest()
        .ok_or_else(binding_unavailable)?;
    state
        .acp
        .start_characterization(
            ActivateSessionCommand {
                project_id: request.project_id,
                chat_id: request.chat_id,
                expected_projection_revision: request.expected_projection_revision,
                client_intent_id: request.client_intent_id,
                session_id: request.session_id,
                runtime_epoch: request.runtime_epoch,
                artifact_digest,
                now_millis: crate::persistence::now_millis(),
            },
            project_root,
        )
        .await
        .map_err(map_error)?;
    mutation_response(state, project_id, chat_id)
}

pub(super) fn send(
    state: &AppState,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let SendAcpTurnRequest::Session {
        project_id,
        project_generation,
        chat_id,
        expected_projection_revision,
        client_intent_id,
        session_id,
        runtime_epoch,
        prompt,
    } = request
    else {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message("Start and activate the characterized session before sending."),
        });
    };
    state
        .acp
        .send_characterization(PrepareTurnCommand {
            project_id: project_id.clone(),
            project_generation,
            chat_id: chat_id.clone(),
            session_id,
            runtime_epoch,
            expected_projection_revision,
            client_intent_id,
            prompt,
            now_millis: crate::persistence::now_millis(),
        })
        .map_err(map_error)?;
    mutation_response(state, project_id, chat_id)
}

pub(super) fn cancel(
    state: &AppState,
    request: CancelAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .cancel_characterization(HostCancelIntent {
            owner: RuntimeOwner {
                project_id: request.project_id,
                chat_id: request.chat_id,
                session_id: request.session_id,
                runtime_epoch: request.runtime_epoch,
            },
            turn_id: request.turn_id,
            client_intent_id: request.client_intent_id,
            now_millis: crate::persistence::now_millis(),
        })
        .map_err(map_error)?;
    mutation_response(state, project_id, chat_id)
}

pub(super) fn close(
    state: &AppState,
    request: CloseAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .close_characterization(CloseSessionCommand {
            project_id: request.project_id,
            chat_id: request.chat_id,
            expected_projection_revision: request.expected_projection_revision,
            client_intent_id: request.client_intent_id,
            session_id: request.session_id,
            runtime_epoch: request.runtime_epoch,
            now_millis: crate::persistence::now_millis(),
        })
        .map_err(map_error)?;
    mutation_response(state, project_id, chat_id)
}

fn binding_unavailable() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::BindingUnavailable,
        message: safe_message("The characterized artifact is not admitted for this launch."),
    }
}

fn map_error(error: CharacterizationError) -> AcpIpcError {
    match error {
        CharacterizationError::Artifact | CharacterizationError::Launch => binding_unavailable(),
        CharacterizationError::Intent => AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            message: safe_message("The characterized session identity is stale or foreign."),
        },
        CharacterizationError::Capacity => AcpIpcError::Capacity {
            resource: AcpCapacityResource::LiveTree,
            limit: LIMITS.process.operational_live_trees,
            message: safe_message("The characterized runtime tree capacity is occupied."),
        },
        CharacterizationError::ProcessRecovery | CharacterizationError::Recovery => {
            AcpIpcError::RecoveryRequired {
                message: safe_message(
                    "The characterized process outcome requires authoritative restart recovery.",
                ),
            }
        }
        CharacterizationError::Session
        | CharacterizationError::Activation
        | CharacterizationError::Observation => AcpIpcError::Internal {
            message: safe_message("The characterized ACP operation failed safely."),
        },
    }
}
