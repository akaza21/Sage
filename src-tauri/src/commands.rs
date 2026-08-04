use tauri::State;

use crate::{
    acp::{
        contract::{ProjectGeneration, SafeText, SageChatId, SageProjectId},
        intent::{
            AcpCapacityResource, AcpChatPage, AcpConflictResource, AcpIpcError,
            AcpMutationResponse, AcpUnavailableReason, ActivateAcpSessionRequest,
            CancelAcpTurnRequest, CloseAcpSessionRequest, GetAcpChatPageRequest, IntentError,
            IntentSnapshot, SendAcpTurnRequest, StartAcpSessionRequest, validate_base_intent,
            validate_cancel_intent, validate_session_intent,
        },
        page_admission::PageAdmissionError,
        store::StoreError,
    },
    domain::{ModelOption, SimulatorTurnResult},
    harnesses::{SIMULATOR_DEFAULT_MODEL_ID, SIMULATOR_ID},
    state::AppState,
};

const MAX_PROMPT_BYTES: usize = 100_000;

fn binding_unavailable() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::BindingUnavailable,
        message: safe_message("No curated ACP harness binding is admitted in this build."),
    }
}

#[tauri::command]
pub async fn get_acp_chat_page(
    state: State<'_, AppState>,
    request: GetAcpChatPageRequest,
) -> Result<AcpChatPage, AcpIpcError> {
    get_acp_chat_page_inner(&state, request).await
}

async fn get_acp_chat_page_inner(
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

fn page_admission_error(error: PageAdmissionError) -> AcpIpcError {
    let limit = match error {
        PageAdmissionError::Owner => {
            crate::acp::limits::LIMITS
                .durable
                .outstanding_page_reads_per_session
        }
        PageAdmissionError::Global => {
            crate::acp::limits::LIMITS
                .durable
                .outstanding_page_reads_global
        }
    };
    AcpIpcError::Capacity {
        resource: AcpCapacityResource::PageRead,
        limit,
        message: safe_message("Too many ACP history pages are already being read."),
    }
}

#[tauri::command]
pub async fn start_acp_session(
    state: State<'_, AppState>,
    request: StartAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    start_acp_session_inner(&state, request).await
}

async fn start_acp_session_inner(
    state: &AppState,
    request: StartAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let snapshot = snapshot_for(
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
    Err(binding_unavailable())
}

#[tauri::command]
pub async fn activate_acp_session(
    state: State<'_, AppState>,
    request: ActivateAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let snapshot = snapshot_for(
        &state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_session_intent(&request, &snapshot).map_err(intent_error)?;
    Err(binding_unavailable())
}

#[tauri::command]
pub async fn send_acp_turn(
    state: State<'_, AppState>,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (project_id, generation, chat_id) = send_owner(&request);
    let snapshot = snapshot_for(&state, project_id.clone(), generation, chat_id.clone()).await?;
    validate_send(&request, &snapshot)?;
    Err(binding_unavailable())
}

#[tauri::command]
pub async fn cancel_acp_turn(
    state: State<'_, AppState>,
    request: CancelAcpTurnRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let snapshot = snapshot_for(
        &state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    validate_cancel_intent(&request, &snapshot).map_err(intent_error)?;
    Err(session_unavailable())
}

#[tauri::command]
pub async fn close_acp_session(
    state: State<'_, AppState>,
    request: CloseAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let snapshot = snapshot_for(
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
    if snapshot.session_id.as_ref() != Some(&request.session_id) {
        return Err(intent_error(IntentError::ForeignSession));
    }
    if snapshot.runtime_epoch != Some(request.runtime_epoch) {
        return Err(intent_error(IntentError::StaleRuntimeEpoch));
    }
    Err(session_unavailable())
}

async fn snapshot_for(
    state: &AppState,
    project_id: SageProjectId,
    generation: ProjectGeneration,
    chat_id: SageChatId,
) -> Result<IntentSnapshot, AcpIpcError> {
    let _transition = lock_acp_context(state, &project_id, generation).await?;
    state
        .acp
        .intent_snapshot(project_id, generation, chat_id)
        .map_err(store_error)
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

fn safe_message(message: &'static str) -> SafeText<8192> {
    SafeText::new("message", message).expect("static ACP message is bounded")
}

fn session_unavailable() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: AcpUnavailableReason::SessionUnavailable,
        message: safe_message("The requested ACP runtime tree is not live."),
    }
}

fn internal_error() -> AcpIpcError {
    AcpIpcError::Internal {
        message: safe_message("The ACP request failed safely."),
    }
}

fn store_error(error: StoreError) -> AcpIpcError {
    match error {
        StoreError::Capacity { limit, .. } => AcpIpcError::Capacity {
            resource: AcpCapacityResource::History,
            limit,
            message: safe_message("Durable ACP history capacity has been reached."),
        },
        StoreError::Conflict { .. } => AcpIpcError::Conflict {
            resource: AcpConflictResource::Session,
            message: safe_message("The requested ACP projection is stale or foreign."),
        },
        StoreError::CommitUnknown => AcpIpcError::RecoveryRequired {
            message: safe_message("The durable outcome is unknown; reload authoritative history."),
        },
        StoreError::Corrupt(_) => AcpIpcError::RecoveryRequired {
            message: safe_message("Durable ACP history failed bounded validation."),
        },
        StoreError::PayloadTooLarge | StoreError::BatchShape => AcpIpcError::InvalidRequest {
            message: safe_message("The ACP request violates its bounded contract."),
        },
        StoreError::Database(_)
        | StoreError::InvalidTransition { .. }
        | StoreError::TerminalConflict { .. }
        | StoreError::CounterExhausted => internal_error(),
    }
}

fn intent_error(error: IntentError) -> AcpIpcError {
    let resource = match error {
        IntentError::ForeignProject | IntentError::StaleProjectGeneration => {
            AcpConflictResource::ProjectGeneration
        }
        IntentError::ForeignChat | IntentError::StaleProjectionRevision => {
            AcpConflictResource::ProjectionRevision
        }
        IntentError::ForeignSession => AcpConflictResource::Session,
        IntentError::StaleRuntimeEpoch => AcpConflictResource::RuntimeEpoch,
        IntentError::ForeignTurn | IntentError::UnexpectedTurnState => {
            AcpConflictResource::TurnState
        }
        IntentError::StaleTurnRevision => AcpConflictResource::TurnRevision,
        IntentError::InvalidProjectionRevision => AcpConflictResource::ProjectionRevision,
    };
    AcpIpcError::Conflict {
        resource,
        message: safe_message("The ACP intent is stale or belongs to another resource."),
    }
}

#[tauri::command]
pub fn list_harness_models(
    state: State<'_, AppState>,
    harness_id: String,
) -> Result<Vec<ModelOption>, String> {
    state
        .harnesses
        .models(&harness_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn send_simulated_prompt(
    state: State<'_, AppState>,
    chat_id: String,
    harness_id: String,
    model_id: Option<String>,
    text: String,
) -> Result<SimulatorTurnResult, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("prompt must not be empty".into());
    }
    if text.len() > MAX_PROMPT_BYTES {
        return Err(format!(
            "prompt exceeds the {MAX_PROMPT_BYTES}-byte Phase 0 limit"
        ));
    }
    if harness_id != SIMULATOR_ID {
        return Err("install and initialize this harness before sending a prompt".into());
    }
    let advertised_models = state
        .harnesses
        .models(&harness_id)
        .map_err(|error| error.to_string())?;
    if let Some(model_id) = &model_id
        && !advertised_models.iter().any(|model| &model.id == model_id)
    {
        return Err("selected model is not advertised by this harness".into());
    }
    let model_id = model_id.as_deref().unwrap_or(SIMULATOR_DEFAULT_MODEL_ID);
    let response = format!(
        "Simulator acknowledged the prompt using {model_id}. The ACP supervisor remains a later phase."
    );
    let _transition = state.project_transitions.lock().await;
    state
        .database
        .append_simulator_turn_for_model(&chat_id, model_id, text, &response)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use uuid::Uuid;

    use super::*;
    use crate::{
        acp::{
            contract::ClientIntentId,
            intent::BindingId,
            projection::{ItemContent, TextRole},
        },
        persistence::Database,
    };

    fn state_with_project() -> (AppState, String, String, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("sage-acp-command-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let chat_id = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        (AppState::new(database), project.id, chat_id, root)
    }

    #[test]
    fn page_read_uses_native_generation_and_projects_v5_history() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (state, project, chat, root) = state_with_project();
            let page = get_acp_chat_page_inner(
                &state,
                GetAcpChatPageRequest {
                    project_id: SageProjectId::parse(project).unwrap(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: SageChatId::parse(chat).unwrap(),
                    cursor: None,
                },
            )
            .await
            .unwrap();
            assert_eq!(page.turns.as_slice().len(), 1);
            assert_eq!(page.turns.as_slice()[0].items.as_slice().len(), 1);
            assert!(matches!(
                &page.turns.as_slice()[0].items.as_slice()[0].content,
                ItemContent::Text {
                    role: TextRole::Assistant,
                    ..
                }
            ));
            drop(state);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn unavailable_binding_is_typed_and_never_mutates_durable_history() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (state, project, chat, root) = state_with_project();
            let project = SageProjectId::parse(project).unwrap();
            let chat = SageChatId::parse(chat).unwrap();
            let page = state
                .acp
                .load_chat_page(project.clone(), chat.clone(), None)
                .unwrap();
            let before: (i64, i64, i64) = state
                .database
                .connection()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM agent_sessions),
                            (SELECT COUNT(*) FROM session_facts),
                            (SELECT projection_revision FROM chats WHERE id=?1)",
                    [chat.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            let result = start_acp_session_inner(
                &state,
                StartAcpSessionRequest {
                    project_id: project,
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat.clone(),
                    expected_projection_revision: page.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: BindingId::new("bindingId", "not-admitted").unwrap(),
                    model_id: None,
                },
            )
            .await;
            assert!(matches!(
                result,
                Err(AcpIpcError::Unavailable {
                    reason: AcpUnavailableReason::BindingUnavailable,
                    ..
                })
            ));
            let after: (i64, i64, i64) = state
                .database
                .connection()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM agent_sessions),
                            (SELECT COUNT(*) FROM session_facts),
                            (SELECT projection_revision FROM chats WHERE id=?1)",
                    [chat.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(after, before);
            drop(state);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn stale_native_generation_is_rejected_before_page_read() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (state, project, chat, root) = state_with_project();
            let result = get_acp_chat_page_inner(
                &state,
                GetAcpChatPageRequest {
                    project_id: SageProjectId::parse(project).unwrap(),
                    project_generation: ProjectGeneration::new(1).unwrap(),
                    chat_id: SageChatId::parse(chat).unwrap(),
                    cursor: None,
                },
            )
            .await;
            assert!(matches!(
                result,
                Err(AcpIpcError::Conflict {
                    resource: AcpConflictResource::ProjectGeneration,
                    ..
                })
            ));
            drop(state);
            fs::remove_dir_all(root).unwrap();
        });
    }
}
