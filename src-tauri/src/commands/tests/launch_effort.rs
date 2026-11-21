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

#[test]
fn a_lost_session_start_is_not_reused_and_the_draft_keeps_its_access_and_effort() {
    use crate::chat_lifecycle::create::ChatCreateStore;
    use sage_acp_protocol::access_mode::SessionAccessMode;
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (mut state, project, chat, root) = state_with_admitted_codex_project();
        // Use the production notifier: the single-owner test mailbox cannot
        // publish a second chat's session after the lost first response.
        struct View;
        impl crate::acp::runtime::NotificationSink for View {
            fn publish<'a>(
                &'a self,
                _: crate::acp::ipc::ProjectionChangedV1,
            ) -> crate::acp::runtime::NotificationSinkFuture<'a> {
                Box::pin(async { Ok(()) })
            }
        }
        state.acp.shutdown_blocking().unwrap();
        state.acp = crate::acp::service::AcpService::new_with_notification_sink_and_registry(
            state.database.clone(),
            Arc::new(View),
            Arc::clone(state.acp.binding_registry()),
        )
        .unwrap();
        let project = SageProjectId::parse(project).unwrap();
        let chat = SageChatId::parse(chat).unwrap();
        list_default_model(&state, &project, &["low", "high"]).await;
        // Native start committed, but the renderer never received its response.
        let lost = start_with(&state, &project, &chat, "high").await.unwrap();
        let old_session = lost.projection.current_session_id.clone().unwrap();
        state
            .settings
            .set_chat_access(old_session.as_str(), SessionAccessMode::WorkspaceWrite, 1)
            .unwrap();
        drop(lost);
        assert!(
            state
                .acp
                .load_chat_page(project.clone(), chat.clone(), None)
                .unwrap()
                .turns
                .as_slice()
                .is_empty()
        );
        assert!(
            crate::chat_lifecycle::visibility::reusable_chat(
                &state.database.connection(),
                project.as_str(),
                &[],
                None,
            )
            .unwrap()
            .is_none()
        );
        let snapshot = state.database.bootstrap_snapshot().unwrap();
        let layout = sage_domain::chat::ProjectLayoutRequest {
            project_id: project.as_str().into(),
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        };
        assert!(
            state
                .database
                .reuse_empty_chat(project.as_str(), &layout, chat.as_str())
                .is_err()
        );
        let fresh = SageChatId::parse(state.database.create_chat(&layout).unwrap()).unwrap();
        assert_ne!(fresh, chat);
        let started = start_with(&state, &project, &fresh, "low").await.unwrap();
        let session = started.projection.current_session_id.unwrap();
        state
            .settings
            .set_chat_access(session.as_str(), SessionAccessMode::ReadOnly, 2)
            .unwrap();
        let choices =
            crate::acp::session_config::choices::load(&state.database.connection(), &session)
                .unwrap();
        let access = crate::acp::bindings::access_policy::turn_mode_for_binding(
            state.acp.binding_registry(),
            "codex-app-server",
            state.settings.chat_access(session.as_str()).unwrap(),
            None,
        )
        .unwrap();
        let wire = draft_prompt_wire(&session, &choices, access);
        assert_eq!(wire["method"], "turn/start");
        assert_eq!(wire["params"]["effort"], "low");
        assert_eq!(wire["params"]["sandboxPolicy"]["type"], "readOnly");
        assert_eq!(wire["params"]["sandboxPolicy"]["networkAccess"], false);
        assert_eq!(
            state.settings.chat_access(old_session.as_str()).unwrap(),
            Some(SessionAccessMode::WorkspaceWrite)
        );
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

fn draft_prompt_wire(
    session: &SageAgentSessionId,
    choices: &[ConfigMutation],
    access: sage_acp_protocol::access_mode::SessionAccessMode,
) -> serde_json::Value {
    use crate::acp::{
        bindings::{
            adapter::{AdapterCommand, EgressResolution, HarnessAdapter},
            codex::{
                CodexRuntime, Platform,
                test_identity::{MANAGED_CODEX_HOME, user_agent},
            },
            runtime::NativeRuntime,
        },
        transport::IncrementalFrameDecoder,
    };
    use serde_json::{Value, json};
    struct Mapper;
    impl crate::acp::bindings::adapter::ResourceMapper for Mapper {
        fn map_existing(
            &self,
            _: &str,
        ) -> Result<
            sage_acp_protocol::intent::SafeResourceRef,
            sage_acp_protocol::intent::UnavailableResourceReason,
        > {
            Err(sage_acp_protocol::intent::UnavailableResourceReason::Foreign)
        }
    }
    let mut runtime = NativeRuntime::new(CodexRuntime::fixture_with_identity(
        Platform::WindowsX86_64,
        [3; 32],
        session.clone(),
    ));
    runtime.bind_launch_choices(choices);
    let send = |runtime: &mut NativeRuntime, command| -> Value {
        let egress = runtime.command(command).unwrap();
        let value = serde_json::from_slice(egress.frame().payload()).unwrap();
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .unwrap();
        value
    };
    let ingest = |runtime: &mut NativeRuntime, value: Value| {
        let frame = IncrementalFrameDecoder::new()
            .push_chunk(format!("{value}\n").as_bytes())
            .unwrap()
            .unwrap();
        let batch = runtime.ingest(&frame, &Mapper).unwrap();
        for egress in batch.egress() {
            runtime
                .resolve_egress(egress.token().clone(), EgressResolution::Written)
                .unwrap();
        }
    };
    let init = send(&mut runtime, AdapterCommand::Initialize);
    ingest(
        &mut runtime,
        json!({"id":init["id"],"result":{
            "userAgent":user_agent("sage", "unknown"),"codexHome":MANAGED_CODEX_HOME,
            "platformFamily":"windows","platformOs":"windows"
        }}),
    );
    let thread = send(
        &mut runtime,
        AdapterCommand::NewSession {
            native_root: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            model: None,
        },
    );
    ingest(
        &mut runtime,
        json!({"id":thread["id"],"result":{
            "thread":{"id":"draft-thread"}, "model":"gpt-5-codex", "reasoningEffort":null
        }}),
    );
    let models = send(&mut runtime, AdapterCommand::ModelPage { cursor: None });
    ingest(
        &mut runtime,
        json!({"id":models["id"],"result":{"data":[{
        "id":"gpt-5-codex", "displayName":"Test model", "isDefault":true,
        "defaultReasoningEffort":"high", "supportedReasoningEfforts":[
            {"reasoningEffort":"low","description":"Quick"},
            {"reasoningEffort":"high","description":"Thorough"}
        ]
    }],"nextCursor":null}}),
    );
    send(
        &mut runtime,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "inspect this project").unwrap(),
            client_message_id: RequiredText::new("client", "draft-first-send").unwrap(),
            access_mode: access,
        },
    )
}
