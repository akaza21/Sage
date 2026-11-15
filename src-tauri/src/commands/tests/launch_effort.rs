//! A thinking level drafted before a chat's session exists is checked
//! against what the binding lists for the model the session starts on.
use super::*;
use sage_acp_protocol::{
    contract::RequiredText,
    session_config::{ConfigId, ConfigMutation, ConfigValue},
};

async fn list_default_model(state: &AppState, project: &SageProjectId, levels: &[&str]) {
    let context = state
        .lock_active_project(project, ProjectGeneration::ZERO)
        .await
        .unwrap();
    let authority = state
        .database
        .codex_project_root_authority(&context)
        .unwrap();
    drop(context);
    let binding = state.acp.codex_registration().refresh().unwrap();
    state
        .acp
        .codex_discovery_manager()
        .seed_session_catalog_for_test(
            &authority,
            binding.binding_digest(),
            sage_acp_protocol::intent::ModelId::new("modelId", "gpt-5-codex").unwrap(),
            levels,
        );
}

async fn start_with(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
    effort: &str,
) -> Result<crate::acp::ipc::AcpMutationResponse, AcpIpcError> {
    let page = state
        .acp
        .load_chat_page(project.clone(), chat.clone(), None)
        .unwrap();
    start_acp_session_inner(
        state,
        StartAcpSessionRequest {
            project_id: project.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat.clone(),
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new("bindingId", crate::acp::bindings::codex::CODEX_BINDING_ID)
                .unwrap(),
            model_id: None,
            effort: Some(RequiredText::new("effort", effort).unwrap()),
        },
    )
    .await
}

fn sessions(state: &AppState) -> i64 {
    state
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn a_drafted_level_the_model_does_not_list_starts_no_session() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project = SageProjectId::parse(project).unwrap();
        let chat = SageChatId::parse(chat).unwrap();
        list_default_model(&state, &project, &["low", "medium"]).await;

        let refused = start_with(&state, &project, &chat, "high")
            .await
            .unwrap_err();
        assert!(
            matches!(&refused, AcpIpcError::Unavailable { .. })
                && format!("{refused:?}").contains("not offered"),
            "{refused:?}"
        );
        assert_eq!(
            sessions(&state),
            0,
            "nothing starts on a level dropped unseen"
        );

        let started = start_with(&state, &project, &chat, "low").await.unwrap();
        let session = started.projection.current_session_id.unwrap();
        assert_eq!(
            crate::acp::session_config::choices::load(&state.database.connection(), &session)
                .unwrap(),
            [ConfigMutation::Option {
                config_id: ConfigId::new("configId", "effort").unwrap(),
                value: ConfigValue::Select {
                    value: ConfigId::new("value", "low").unwrap(),
                },
            }]
        );
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}
