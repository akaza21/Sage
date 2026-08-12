#[cfg(all(windows, feature = "acp-fixture"))]
mod characterization;
pub(crate) mod codex_auth;
mod errors;
mod session_routes;

pub(crate) use session_routes::{activate_acp_session_inner, start_acp_session_inner};

use errors::{
    binding_unavailable, codex_discovery_error, codex_session_error, intent_error, internal_error,
    page_admission_error, safe_message, store_error,
};

use tauri::State;

use crate::{
    acp::{
        bindings::codex::CodexDiscoveryDeadline,
        contract::{ProjectGeneration, SageChatId, SageProjectId},
        intent::{
            AcpChatPage, AcpIpcError, AcpMutationResponse, ActivateAcpSessionRequest,
            CancelAcpTurnRequest, CloseAcpSessionRequest, CodexDiscoveryResponse,
            GetAcpChatPageRequest, GetCodexDiscoveryRequest, IntentError, IntentSnapshot,
            SendAcpTurnRequest, StartAcpSessionRequest, validate_base_intent,
            validate_cancel_intent, validate_session_intent,
        },
        runtime::HostCancelIntent,
        scheduler::RuntimeOwner,
        store::{
            ActivateSessionCommand, CloseSessionCommand, PrepareTurnCommand, StartSessionCommand,
        },
    },
    state::{ActiveProjectContext, ActiveProjectError, AppState},
};

#[tauri::command]
pub async fn get_codex_discovery(
    state: State<'_, AppState>,
    request: GetCodexDiscoveryRequest,
) -> Result<CodexDiscoveryResponse, AcpIpcError> {
    get_codex_discovery_inner(&state, request).await
}

pub(crate) async fn get_codex_discovery_inner(
    state: &AppState,
    request: GetCodexDiscoveryRequest,
) -> Result<CodexDiscoveryResponse, AcpIpcError> {
    let deadline = CodexDiscoveryDeadline::new().map_err(codex_discovery_error)?;
    get_codex_discovery_with_deadline(state, request, deadline).await
}

pub(crate) async fn get_codex_discovery_with_deadline(
    state: &AppState,
    request: GetCodexDiscoveryRequest,
    deadline: CodexDiscoveryDeadline,
) -> Result<CodexDiscoveryResponse, AcpIpcError> {
    let transition = tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline.operation_end()),
        lock_acp_context(state, &request.project_id, request.project_generation),
    )
    .await
    .map_err(|_| {
        codex_discovery_error(crate::acp::bindings::codex::CodexDiscoveryRunError::Timeout)
    })??;
    let project_root = state
        .database
        .codex_project_root_authority(&transition)
        .map_err(|_| AcpIpcError::Unavailable {
            reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
            message: safe_message("Codex beta requires one unchanged Project root."),
        })?;
    let manager = state.acp.codex_discovery_manager();
    let prepared = manager
        .prepare(request, project_root, deadline)
        .map_err(codex_discovery_error)?;
    drop(transition);
    tauri::async_runtime::spawn_blocking(move || manager.discover(prepared))
        .await
        .map_err(|_| internal_error())?
        .map_err(codex_discovery_error)
}

#[tauri::command]
pub async fn get_acp_chat_page(
    state: State<'_, AppState>,
    request: GetAcpChatPageRequest,
) -> Result<AcpChatPage, AcpIpcError> {
    get_acp_chat_page_inner(&state, request).await
}

pub(crate) async fn get_acp_chat_page_inner(
    state: &AppState,
    request: GetAcpChatPageRequest,
) -> Result<AcpChatPage, AcpIpcError> {
    let _page_read = state
        .acp
        .reserve_page_read(&request.project_id, &request.chat_id)
        .map_err(page_admission_error)?;
    let _transition =
        lock_acp_context(state, &request.project_id, request.project_generation).await?;
    let authoritative_bootstrap = request.cursor.is_none();
    let page = state
        .acp
        .load_chat_page(request.project_id, request.chat_id, request.cursor)
        .map_err(store_error)?;
    if authoritative_bootstrap {
        state.acp.acknowledge_projection_bootstrap();
    }
    Ok(page)
}

#[tauri::command]
pub async fn start_acp_session(
    state: State<'_, AppState>,
    request: StartAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    start_acp_session_inner(&state, request).await
}

#[tauri::command]
pub async fn activate_acp_session(
    state: State<'_, AppState>,
    request: ActivateAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    activate_acp_session_inner(&state, request).await
}

#[tauri::command]
pub async fn send_acp_turn(
    state: State<'_, AppState>,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    send_acp_turn_inner(&state, request).await
}

pub(crate) async fn send_acp_turn_inner(
    state: &AppState,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (project_id, generation, chat_id) = send_owner(&request);
    let (_transition, snapshot) =
        snapshot_for(state, project_id.clone(), generation, chat_id.clone()).await?;
    validate_send(&request, &snapshot)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.characterization_enabled() {
        return characterization::send(state, request);
    }
    if let SendAcpTurnRequest::Session {
        project_id,
        project_generation,
        chat_id,
        expected_projection_revision,
        client_intent_id,
        session_id,
        runtime_epoch,
        prompt,
    } = request
    {
        state
            .acp
            .send_codex_turn(PrepareTurnCommand {
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
            .map_err(codex_session_error)?;
        return mutation_response(state, project_id, chat_id);
    }
    Err(binding_unavailable())
}

#[tauri::command]
pub async fn cancel_acp_turn(
    state: State<'_, AppState>,
    request: CancelAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    cancel_acp_turn_inner(&state, request).await
}

pub(crate) async fn cancel_acp_turn_inner(
    state: &AppState,
    request: CancelAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_cancel_intent(&request, &snapshot).map_err(intent_error)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.characterization_enabled() {
        return characterization::cancel(state, request);
    }
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .cancel_codex_turn(HostCancelIntent {
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
        .map_err(codex_session_error)?;
    mutation_response(state, project_id, chat_id)
}

#[tauri::command]
pub async fn close_acp_session(
    state: State<'_, AppState>,
    request: CloseAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    close_acp_session_inner(&state, request).await
}

pub(crate) async fn close_acp_session_inner(
    state: &AppState,
    request: CloseAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
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
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(intent_error(IntentError::ForeignSession));
    }
    if snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(intent_error(IntentError::StaleRuntimeEpoch));
    }
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.characterization_enabled() {
        return characterization::close(state, request);
    }
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .close_codex_session(CloseSessionCommand {
            project_id: request.project_id,
            chat_id: request.chat_id,
            expected_projection_revision: request.expected_projection_revision,
            client_intent_id: request.client_intent_id,
            session_id: request.session_id,
            runtime_epoch: request.runtime_epoch,
            now_millis: crate::persistence::now_millis(),
        })
        .map_err(codex_session_error)?;
    mutation_response(state, project_id, chat_id)
}

async fn snapshot_for<'a>(
    state: &'a AppState,
    project_id: SageProjectId,
    generation: ProjectGeneration,
    chat_id: SageChatId,
) -> Result<(ActiveProjectContext<'a>, IntentSnapshot), AcpIpcError> {
    let transition = lock_acp_context(state, &project_id, generation).await?;
    let snapshot = state
        .acp
        .intent_snapshot(project_id, generation, chat_id)
        .map_err(store_error)?;
    Ok((transition, snapshot))
}

async fn lock_acp_context<'a>(
    state: &'a AppState,
    project_id: &SageProjectId,
    generation: ProjectGeneration,
) -> Result<ActiveProjectContext<'a>, AcpIpcError> {
    state
        .lock_active_project(project_id, generation)
        .await
        .map_err(|error| match error {
            ActiveProjectError::GenerationUnavailable => AcpIpcError::RecoveryRequired {
                message: safe_message("The native project generation is unavailable; reload Sage."),
            },
            ActiveProjectError::StaleGeneration => {
                intent_error(IntentError::StaleProjectGeneration)
            }
            ActiveProjectError::ForeignProject => intent_error(IntentError::ForeignProject),
            ActiveProjectError::Storage => internal_error(),
        })
}

fn send_owner(request: &SendAcpTurnRequest) -> (&SageProjectId, ProjectGeneration, &SageChatId) {
    match request {
        SendAcpTurnRequest::Session {
            project_id,
            project_generation,
            chat_id,
            ..
        }
        | SendAcpTurnRequest::NewSession {
            project_id,
            project_generation,
            chat_id,
            ..
        } => (project_id, *project_generation, chat_id),
    }
}

fn validate_send(
    request: &SendAcpTurnRequest,
    snapshot: &IntentSnapshot,
) -> Result<(), AcpIpcError> {
    match request {
        SendAcpTurnRequest::Session {
            project_id,
            project_generation,
            chat_id,
            expected_projection_revision,
            session_id,
            runtime_epoch,
            ..
        } => {
            validate_base_intent(
                project_id,
                *project_generation,
                chat_id,
                *expected_projection_revision,
                snapshot,
            )
            .map_err(intent_error)?;
            if snapshot.session_id.as_ref() != Some(session_id) {
                return Err(intent_error(IntentError::ForeignSession));
            }
            if snapshot.runtime_epoch != Some(*runtime_epoch) {
                return Err(intent_error(IntentError::StaleRuntimeEpoch));
            }
            Ok(())
        }
        SendAcpTurnRequest::NewSession {
            project_id,
            project_generation,
            chat_id,
            expected_projection_revision,
            ..
        } => validate_base_intent(
            project_id,
            *project_generation,
            chat_id,
            *expected_projection_revision,
            snapshot,
        )
        .map_err(intent_error),
    }
}

pub(super) fn mutation_response(
    state: &AppState,
    project_id: SageProjectId,
    chat_id: SageChatId,
) -> Result<AcpMutationResponse, AcpIpcError> {
    state
        .acp
        .load_chat_page(project_id, chat_id, None)
        .map(|projection| AcpMutationResponse { projection })
        .map_err(store_error)
}
