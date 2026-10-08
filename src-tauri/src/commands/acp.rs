#[cfg(all(windows, feature = "acp-fixture"))]
mod characterization;
pub(crate) mod claude_auth;
pub(crate) mod codex_auth;
pub(crate) mod edit_disposition;
mod errors;
pub(crate) mod file_reference;
pub(crate) mod goal;
pub(crate) mod permission;
pub(crate) mod read_text_file;
#[cfg(any(debug_assertions, feature = "diagnostics"))]
pub(crate) mod runtime_identity;
pub(crate) mod session_config;
mod session_operations;
mod session_routes;

pub(crate) use session_routes::{activate_acp_session_inner, start_acp_session_inner};

use errors::{
    binding_unavailable, codex_discovery_error, dispatch_error, intent_error, internal_error,
    page_admission_error, safe_message, session_error, store_error,
};

use tauri::State;

use crate::{
    acp::{
        attached_context::AttachedContexts,
        bindings::codex::CodexDiscoveryDeadline,
        contract::{ProjectGeneration, SageChatId, SageProjectId},
        intent::{
            AcpChatPage, AcpFileSnapshotResponse, AcpIpcError, AcpMutationResponse,
            ActivateAcpSessionRequest, CancelAcpTurnRequest, CloseAcpSessionRequest,
            CodexDiscoveryResponse, GetAcpChatPageRequest, GetAcpFileSnapshotRequest,
            GetCodexDiscoveryRequest, IntentError, IntentSnapshot, PromptDispatch,
            SendAcpTurnFailure, SendAcpTurnRequest, StartAcpSessionRequest, SteerAcpTurnRequest,
            validate_base_intent, validate_cancel_intent, validate_session_intent,
            validate_steer_intent,
        },
        runtime::{HostCancelIntent, HostSteerIntent},
        scheduler::RuntimeOwner,
        store::{
            ActivateSessionCommand, CloseSessionCommand, PrepareTurnCommand, StartSessionCommand,
            read_snapshot,
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
    let discovery_manager = manager.clone();
    let task = tauri::async_runtime::spawn_blocking(move || manager.discover(prepared));
    await_codex_discovery(task, deadline, move || discovery_manager.invalidate()).await
}

pub(crate) async fn await_codex_discovery(
    task: tauri::async_runtime::JoinHandle<
        Result<CodexDiscoveryResponse, crate::acp::bindings::codex::CodexDiscoveryRunError>,
    >,
    deadline: CodexDiscoveryDeadline,
    on_timeout: impl FnOnce(),
) -> Result<CodexDiscoveryResponse, AcpIpcError> {
    match tokio::time::timeout_at(tokio::time::Instant::from_std(deadline.cleanup_end()), task)
        .await
    {
        Ok(result) => result
            .map_err(|_| internal_error())?
            .map_err(codex_discovery_error),
        Err(_) => {
            on_timeout();
            Err(codex_discovery_error(
                crate::acp::bindings::codex::CodexDiscoveryRunError::Timeout,
            ))
        }
    }
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
pub async fn get_acp_file_snapshot(
    state: State<'_, AppState>,
    request: GetAcpFileSnapshotRequest,
) -> Result<AcpFileSnapshotResponse, AcpIpcError> {
    get_acp_file_snapshot_inner(&state, request).await
}

mod file_snapshot;
pub(crate) use file_snapshot::get_acp_file_snapshot_inner;

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
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    send_acp_turn_inner(&state, request).await
}

pub(crate) use session_operations::{
    cancel_acp_turn_inner, close_acp_session_inner, end_acp_session_inner, send_acp_turn_inner,
    steer_acp_turn_inner,
};

#[tauri::command]
pub async fn cancel_acp_turn(
    state: State<'_, AppState>,
    request: CancelAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    cancel_acp_turn_inner(&state, request).await
}

#[tauri::command]
pub async fn steer_acp_turn(
    state: State<'_, AppState>,
    request: SteerAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    steer_acp_turn_inner(&state, request).await
}

#[tauri::command]
pub async fn close_acp_session(
    state: State<'_, AppState>,
    request: CloseAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    close_acp_session_inner(&state, request).await
}

#[tauri::command]
pub async fn end_acp_session(
    state: State<'_, AppState>,
    request: CloseAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    end_acp_session_inner(&state, request).await
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
    state: &AppState,
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
            attached_context,
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
            validate_attached_context(
                state,
                project_id,
                chat_id,
                *expected_projection_revision,
                attached_context,
            )
        }
        SendAcpTurnRequest::NewSession {
            project_id,
            project_generation,
            chat_id,
            expected_projection_revision,
            attached_context,
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
            validate_attached_context(
                state,
                project_id,
                chat_id,
                *expected_projection_revision,
                attached_context,
            )
        }
    }
}

fn validate_attached_context(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    expected_revision: crate::acp::contract::ProjectionRevision,
    contexts: &AttachedContexts,
) -> Result<(), AcpIpcError> {
    if contexts.as_slice().iter().all(|context| {
        matches!(
            context,
            crate::acp::attached_context::AttachedContext::BrowserPage(_)
        )
    }) {
        return Ok(());
    }
    let connection = state.database.connection();
    let mut statement = connection
        .prepare("SELECT id FROM project_roots WHERE project_id=?1 ORDER BY ordinal,id LIMIT 9")
        .map_err(|_| internal_error())?;
    let roots = statement
        .query_map([project_id.as_str()], |row| row.get::<_, String>(0))
        .map_err(|_| internal_error())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| internal_error())?;
    if roots.is_empty() || roots.len() > 8 {
        return Err(internal_error());
    }
    let roots = roots
        .into_iter()
        .map(|root| crate::acp::contract::SageRootId::parse(root).map_err(|_| internal_error()))
        .collect::<Result<Vec<_>, _>>()?;
    for context in contexts.as_slice() {
        let crate::acp::attached_context::AttachedContext::DiffLineComment(context) = context
        else {
            continue;
        };
        let reference = &context.diff_ref;
        if reference.project_id != project_id.as_str() || reference.chat_id != chat_id.as_str() {
            return Err(AcpIpcError::InvalidRequest {
                message: safe_message("Attached context belongs to a different project or Chat."),
            });
        }
        let snapshot =
            crate::acp::service::verify_current_owner(&connection, expected_revision, reference)
                .and_then(|()| read_snapshot(&connection, reference, &roots))
                .map_err(|error| match error {
                    crate::acp::file_diff::SnapshotReadError::Stale
                    | crate::acp::file_diff::SnapshotReadError::Unavailable => {
                        AcpIpcError::Conflict {
                            resource: crate::acp::intent::AcpConflictResource::Session,
                            message: safe_message(
                                "Attached context belongs to an unavailable file snapshot.",
                            ),
                        }
                    }
                    crate::acp::file_diff::SnapshotReadError::Forbidden => {
                        AcpIpcError::InvalidRequest {
                            message: safe_message(
                                "Attached context is not available to the current owner.",
                            ),
                        }
                    }
                    crate::acp::file_diff::SnapshotReadError::Corrupt => {
                        AcpIpcError::RecoveryRequired {
                            message: safe_message("Attached context failed its integrity check."),
                        }
                    }
                })?;
        context
            .validate_line_exists(&snapshot.old_text, &snapshot.new_text)
            .map_err(|_| AcpIpcError::InvalidRequest {
                message: safe_message("Attached context line range is outside the saved snapshot."),
            })?;
    }
    Ok(())
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
