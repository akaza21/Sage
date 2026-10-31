use crate::{
    acp::{
        attached_context::effective_prompt,
        contract::SafeText,
        intent::{
            AcpCapacityResource, AcpConflictResource, AcpIpcError, AcpMutationResponse,
            AcpUnavailableReason, ActivateAcpSessionRequest, CancelAcpTurnRequest,
            CloseAcpSessionRequest, SendAcpTurnFailure, SendAcpTurnRequest, StartAcpSessionRequest,
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

use super::{
    errors::safe_message,
    mutation_response,
    session_operations::{prompt_dispatch_unknown, prompt_not_dispatched},
};

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

fn binding_unavailable() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::BindingUnavailable,
        message: safe_message("The characterized artifact is not admitted for this launch."),
    }
}

pub(super) fn transition(
    state: &AppState,
    request: crate::acp::intent::TransitionAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let project = request.project_id.clone();
    let chat = request.chat_id.clone();
    state
        .acp
        .transition_characterization_session(
            crate::acp::store::session_replace::ReplaceSessionCommand {
                expected_head: request.expected_head,
                confirm_unknown_delivery: request.confirm_unknown_delivery,
                start: StartSessionCommand {
                    project_id: request.project_id,
                    chat_id: request.chat_id,
                    expected_projection_revision: request.expected_projection_revision,
                    client_intent_id: request.client_intent_id,
                    binding_id: request.binding_id,
                    binding_digest: CHARACTERIZATION_BINDING_DIGEST.to_owned(),
                    harness_version: Some(
                        SafeText::new("harnessVersion", "fixture-1")
                            .expect("static fixture version"),
                    ),
                    model_id: request.model_id,
                    now_millis: crate::persistence::now_millis(),
                },
            },
        )
        .map_err(map_error)?;
    mutation_response(state, project, chat)
}

pub(super) fn send(
    state: &AppState,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    let SendAcpTurnRequest::Session {
        project_id,
        project_generation,
        chat_id,
        expected_projection_revision,
        client_intent_id,
        session_id,
        runtime_epoch,
        prompt,
        attached_context,
    } = request
    else {
        return Err(prompt_not_dispatched(AcpIpcError::InvalidRequest {
            message: safe_message("Start and activate the characterized session before sending."),
        }));
    };
    let prompt = effective_prompt(&prompt, &attached_context)
        .map_err(|_| AcpIpcError::InvalidRequest {
            message: safe_message("The prompt and attached context exceed the native byte limit."),
        })
        .map_err(prompt_not_dispatched)?;
    let mut command = PrepareTurnCommand {
        project_id: project_id.clone(),
        project_generation,
        chat_id: chat_id.clone(),
        session_id,
        runtime_epoch,
        expected_projection_revision,
        client_intent_id,
        prompt,
        now_millis: crate::persistence::now_millis(),
    };
    state
        .acp
        .freeze_handoff_prompt(&mut command)
        .map_err(super::errors::store_error)
        .map_err(prompt_not_dispatched)?;
    state
        .acp
        .send_characterization(command)
        .map_err(|failure| SendAcpTurnFailure {
            failure: map_error(failure.error),
            prompt_dispatch: failure.prompt_dispatch,
        })?;
    mutation_response(state, project_id, chat_id).map_err(prompt_dispatch_unknown)
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

pub(super) fn map_error(error: CharacterizationError) -> AcpIpcError {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        contract::{
            ClientIntentId, ProjectGeneration, RuntimeEpoch, SageAgentSessionId, SageChatId,
            SageProjectId,
        },
        intent::{Prompt, PromptDispatch},
    };

    #[test]
    fn characterization_missing_runtime_proves_prompt_not_started() {
        let (state, project, chat, root) = crate::commands::tests::state_with_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let failure = send(
            &state,
            SendAcpTurnRequest::Session {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: SageAgentSessionId::parse("unavailable-session").unwrap(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                prompt: Prompt::new("prompt", "Do not infer retry safety from an error kind")
                    .unwrap(),
                attached_context: Default::default(),
            },
        )
        .unwrap_err();
        assert_eq!(failure.prompt_dispatch, PromptDispatch::NotStarted);
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }
}
