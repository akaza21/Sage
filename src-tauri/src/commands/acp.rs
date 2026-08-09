#[cfg(all(windows, feature = "acp-fixture"))]
mod characterization;
mod errors;

use errors::{
    binding_unavailable, intent_error, internal_error, page_admission_error, safe_message,
    session_unavailable, store_error,
};

use tauri::State;

use crate::{
    acp::{
        contract::{ProjectGeneration, SageChatId, SageProjectId},
        intent::{
            AcpChatPage, AcpIpcError, AcpMutationResponse, ActivateAcpSessionRequest,
            CancelAcpTurnRequest, CloseAcpSessionRequest, GetAcpChatPageRequest, IntentError,
            IntentSnapshot, SendAcpTurnRequest, StartAcpSessionRequest, validate_base_intent,
            validate_cancel_intent, validate_session_intent,
        },
    },
    state::AppState,
};

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

pub(crate) async fn start_acp_session_inner(
    state: &AppState,
    request: StartAcpSessionRequest,
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
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&request.binding_id) {
        return characterization::start(state, request).await;
    }
    Err(binding_unavailable())
}

#[tauri::command]
pub async fn activate_acp_session(
    state: State<'_, AppState>,
    request: ActivateAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    activate_acp_session_inner(&state, request).await
}

pub(crate) async fn activate_acp_session_inner(
    state: &AppState,
    request: ActivateAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (_transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_session_intent(&request, &snapshot).map_err(intent_error)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.characterization_enabled() {
        return characterization::activate(state, request).await;
    }
    Err(binding_unavailable())
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
    Err(session_unavailable())
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
    Err(session_unavailable())
}

async fn snapshot_for<'a>(
    state: &'a AppState,
    project_id: SageProjectId,
    generation: ProjectGeneration,
    chat_id: SageChatId,
) -> Result<(tokio::sync::MutexGuard<'a, u64>, IntentSnapshot), AcpIpcError> {
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
) -> Result<tokio::sync::MutexGuard<'a, u64>, AcpIpcError> {
    let transition = state.project_transitions.lock().await;
    let expected =
        ProjectGeneration::new(*transition).map_err(|_| AcpIpcError::RecoveryRequired {
            message: safe_message("The native project generation is unavailable; reload Sage."),
        })?;
    if generation != expected {
        return Err(intent_error(IntentError::StaleProjectGeneration));
    }
    let active = state
        .database
        .active_project_id()
        .map_err(|_| internal_error())?;
    if active.as_deref() != Some(project_id.as_str()) {
        return Err(intent_error(IntentError::ForeignProject));
    }
    Ok(transition)
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
