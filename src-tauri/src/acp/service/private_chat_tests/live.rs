use super::*;
use crate::acp::{
    bindings::codex::CodexProjectRootAuthority,
    ipc::ProjectionChangedV1,
    runtime::{HostPromptIntent, NotificationSink, NotificationSinkFuture},
    store::PrepareTurnCommand,
    transcript_quote::Prompt,
};
use sage_acp_protocol::lifecycle::{SessionState, TurnState};
use std::sync::Mutex;

#[derive(Default)]
struct ChatView(Mutex<Option<ProjectionChangedV1>>);

impl NotificationSink for ChatView {
    fn publish<'a>(&'a self, event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            *self.0.lock().unwrap() = Some(event);
            Ok(())
        })
    }
}

#[test]
fn private_chat_fixture_answer_reaches_the_view_after_command_creates_its_workspace() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let directory = std::env::current_dir()
            .unwrap()
            .join("work")
            .join(format!("private-chat-live-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(directory.join("project")).unwrap();
        let database = crate::database::open_database(&directory.join("sage.db")).unwrap();
        database
            .create_project_with_chat(&directory.join("project"))
            .unwrap();
        let current = database.bootstrap_snapshot().unwrap();
        let current = sage_domain::chat::ProjectLayoutRequest {
            project_id: current.project.unwrap().id,
            tabs: current.tabs,
            active_tab_id: current.active_tab_id,
            selected_root_id: current.selected_root_id,
            selected_resource: current.selected_resource,
        };
        let sink = Arc::new(ChatView::default());
        let registry = Arc::new(BindingRegistry::with_codex(
            CodexBindingRegistration::admitted_for_journey_test(&["private-chat-cwd"]),
        ));
        let mut state = crate::state::AppState::new(database.clone());
        state.acp.shutdown_blocking().unwrap();
        state.acp = AcpService::new_with_notification_sink_and_registry(
            database.clone(),
            sink.clone(),
            registry,
        )
        .unwrap();
        let chat = uuid::Uuid::new_v4().to_string();
        let created = crate::chat_commands::create_chat_inner(
            &state,
            Some(current),
            0,
            None,
            Some(chat.clone()),
        )
        .await;
        assert!(created.committed, "{:?}", created.recovery_error);
        assert!(created.recovery_error.is_none());
        let project = created.snapshot.unwrap().project.unwrap();
        // Existing private Chats saved both the verbatim root and its old key.
        // Exercise that durable input through the real session launch too.
        let record = database.root_records(&project.id).unwrap().remove(0);
        let legacy = record.absolute_path.canonicalize().unwrap();
        database
            .connection()
            .execute(
                "UPDATE project_roots SET absolute_path=?1,canonical_key=?2 WHERE id=?3",
                rusqlite::params![
                    legacy.to_str().unwrap(),
                    legacy.to_string_lossy().to_lowercase(),
                    record.id
                ],
            )
            .unwrap();
        let project_id = SageProjectId::parse(&project.id).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let generation = ProjectGeneration::new(created.transition_epoch).unwrap();
        let context = state
            .lock_active_project(&project_id, generation)
            .await
            .unwrap();
        let root = database.selected_project_root_authority(&context).unwrap();
        let native_root = BindingProjectRoot::Codex(CodexProjectRootAuthority::for_test(root));
        let cwd = native_root.native_root().unwrap();
        assert!(!cwd.starts_with(r"\\?\"), "session cwd: {cwd}");
        let folder = directory.join("chat-workspaces").join(&project.id);
        assert_eq!(
            crate::project_registry::authority_path_key(
                &std::path::Path::new(&cwd).canonicalize().unwrap()
            ),
            crate::project_registry::authority_path_key(&folder.canonicalize().unwrap()),
        );
        super::super::tests::seed_codex_discovery(
            &state.acp,
            native_root.codex_project_root().unwrap(),
            None,
        );
        drop(context);
        let page = || {
            state
                .acp
                .load_chat_page(project_id.clone(), chat_id.clone(), None)
                .unwrap()
        };
        let started = state
            .acp
            .start_session(
                state
                    .acp
                    .binding_registry()
                    .resolve("codex-app-server")
                    .unwrap(),
                &native_root,
                StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: page().projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
                    binding_digest: String::new(),
                    harness_version: None,
                    model_id: None,
                    now_millis: sage_core::time::now_millis(),
                },
            )
            .await
            .unwrap();
        let owner = state
            .acp
            .activate_session(
                state
                    .acp
                    .binding_registry()
                    .resolve("codex-app-server")
                    .unwrap(),
                ActivateSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: page().projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    session_id: started.session_id,
                    runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                    artifact_digest: String::new(),
                    now_millis: sage_core::time::now_millis(),
                },
                native_root,
            )
            .await
            .unwrap();
        let deadline = std::time::Instant::now() + sage_test_support::test_wait::BUDGET;
        while !page()
            .sessions
            .as_slice()
            .iter()
            .any(|session| session.state == SessionState::Ready)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "fixture session did not become ready"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        state
            .acp
            .send_session_turn(HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id: project_id.clone(),
                    project_generation: generation,
                    chat_id: chat_id.clone(),
                    session_id: owner.session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    expected_projection_revision: page().projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    prompt: Prompt::new("prompt", "Reply with the single word ok.").unwrap(),
                    now_millis: sage_core::time::now_millis(),
                },
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
            })
            .unwrap();
        let deadline = std::time::Instant::now() + sage_test_support::test_wait::BUDGET;
        while !page()
            .turns
            .as_slice()
            .iter()
            .any(|turn| turn.state == TurnState::Completed)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "fixture turn did not complete"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(page().turns.as_slice().iter().any(|turn| turn.items.as_slice().iter().any(|item| {
            matches!(&item.content, crate::acp::projection::ItemContent::Text { text, .. } if text.as_str() == "ok")
        })), "fixture did not return its answer");
        let completed_revision = page().projection_revision;
        while !sink.0.lock().unwrap().as_ref().is_some_and(|event| {
            event.project_id == project_id
                && event.chat_id == chat_id
                && event.projection_revision >= completed_revision
        }) && std::time::Instant::now() < deadline
        {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let delivered = sink.0.lock().unwrap().clone();
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        drop(database);
        fs::remove_dir_all(directory).unwrap();
        assert!(
            delivered.is_some_and(|event| {
                event.project_id == project_id
                    && event.chat_id == chat_id
                    && event.projection_revision >= completed_revision
            }),
            "completed fixture answer was durable but never reached the Chat view"
        );
    });
}
