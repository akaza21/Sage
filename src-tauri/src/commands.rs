pub(crate) mod acp;

use tauri::State;

use crate::{
    domain::{ModelOption, SimulatorTurnResult},
    harnesses::{SIMULATOR_DEFAULT_MODEL_ID, SIMULATOR_ID},
    state::AppState,
};

const MAX_PROMPT_BYTES: usize = 100_000;

#[tauri::command]
pub fn list_harness_models(
    state: State<'_, AppState>,
    harness_id: String,
) -> Result<Vec<ModelOption>, String> {
    if harness_id == "codex" {
        let _ = state.harnesses.refresh_codex_registration();
    }
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

    use super::{
        acp::{
            get_acp_chat_page_inner, get_codex_discovery_inner, get_codex_discovery_with_deadline,
            start_acp_session_inner,
        },
        *,
    };
    use crate::{
        acp::{
            bindings::codex::CodexDiscoveryDeadline,
            contract::{ClientIntentId, ProjectGeneration, SageChatId, SageProjectId},
            intent::{
                AcpConflictResource, AcpIpcError, AcpUnavailableReason, BindingId,
                GetAcpChatPageRequest, GetCodexDiscoveryRequest, StartAcpSessionRequest,
            },
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
    fn dark_codex_discovery_is_typed_and_does_not_mutate_sqlite() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (state, project, chat, root) = state_with_project();
            let project = SageProjectId::parse(project).unwrap();
            let before: (i64, i64, i64) = state
                .database
                .connection()
                .query_row(
                    "SELECT (SELECT COUNT(*) FROM agent_sessions),
                            (SELECT COUNT(*) FROM session_facts),
                            (SELECT projection_revision FROM chats WHERE id=?1)",
                    [&chat],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();

            let result = get_codex_discovery_inner(
                &state,
                GetCodexDiscoveryRequest {
                    project_id: project,
                    project_generation: ProjectGeneration::ZERO,
                    binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
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
                    [&chat],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert_eq!(after, before);
            drop(state);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn codex_discovery_deadline_bounds_the_project_transition_wait() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let (state, project, _chat, root) = state_with_project();
            let project = SageProjectId::parse(project).unwrap();
            let held_transition = state.project_transitions.lock().await;
            let request = GetCodexDiscoveryRequest {
                project_id: project.clone(),
                project_generation: ProjectGeneration::ZERO,
                binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
            };
            let started = std::time::Instant::now();

            let result = get_codex_discovery_with_deadline(
                &state,
                request.clone(),
                CodexDiscoveryDeadline::for_test(
                    std::time::Duration::from_millis(25),
                    std::time::Duration::from_millis(25),
                ),
            )
            .await;

            assert!(started.elapsed() < std::time::Duration::from_millis(500));
            assert!(matches!(
                result,
                Err(AcpIpcError::Unavailable {
                    reason: AcpUnavailableReason::CapabilityUnavailable,
                    ..
                })
            ));
            drop(held_transition);
            assert!(matches!(
                get_codex_discovery_inner(&state, request).await,
                Err(AcpIpcError::Unavailable {
                    reason: AcpUnavailableReason::BindingUnavailable,
                    ..
                })
            ));
            drop(state);
            fs::remove_dir_all(root).unwrap();
        });
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
