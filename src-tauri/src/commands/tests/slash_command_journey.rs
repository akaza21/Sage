//! The `/` menu's command route against the `claude-slash-commands` fixture
//! subprocess: the fixture advertises `compact`, `review` (with an input hint)
//! and `init`, then refuses any prompt whose wire text differs from
//! `/review 42`, `/compact` and a plain prompt, in that order.

use super::*;
use crate::acp::bindings::registry::BindingRegistry;
use crate::commands::acp::slash_commands::{RunCommandRequest, run};
use crate::{acp::contract::RequiredText, launch_profile::FixtureSelection};

fn fixture_selection(directory: &Path, case: &str) -> FixtureSelection {
    let profile = directory.join("profile");
    let artifact = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("acp-adversarial-fixture.exe");
    let hash = crate::harness_discovery::sha256_file(&artifact)
        .expect("coordinator must build the acp-fixture executable");
    let mut sha256 = [0; 32];
    for (index, byte) in sha256.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16).unwrap();
    }
    FixtureSelection {
        proof_root: directory.canonicalize().unwrap(),
        profile_root: profile.canonicalize().unwrap(),
        durable_dir: profile.join("durable").canonicalize().unwrap(),
        project_root: directory.join("project").canonicalize().unwrap(),
        artifact_bytes: fs::metadata(&artifact).unwrap().len(),
        artifact_path: artifact.canonicalize().unwrap(),
        artifact_sha256: sha256,
        case: case.into(),
    }
}

fn completed_turns(state: &AppState, owner: &RuntimeOwner) -> usize {
    state
        .acp
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap()
        .turns
        .as_slice()
        .iter()
        .filter(|turn| turn.session_id == owner.session_id && turn.state == TurnState::Completed)
        .count()
}

fn command(
    state: &AppState,
    owner: &RuntimeOwner,
    revision: u64,
    name: &str,
    input: Option<&str>,
) -> RunCommandRequest {
    let page = state
        .acp
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap();
    RunCommandRequest {
        session: ActivateAcpSessionRequest {
            project_id: owner.project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
        },
        expected_commands_revision: revision,
        name: RequiredText::new("name", name).unwrap(),
        input: input.map(|text| RequiredText::new("input", text).unwrap()),
    }
}

#[test]
fn claude_slash_commands_run_through_the_command_route_with_the_live_revision() {
    let _serial = crate::test_wait::serial_process_guard();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let directory = std::env::temp_dir().join(format!("sage-slash-journey-{}", Uuid::new_v4()));
        fs::create_dir_all(directory.join("project/src")).unwrap();
        fs::create_dir_all(directory.join("profile/durable")).unwrap();
        fs::write(
            directory.join("profile/.sage-disposable-profile-v1"),
            b"sage-disposable-profile-v1\n",
        )
        .unwrap();
        let selection = fixture_selection(&directory, "claude-slash-commands");
        let database = Database::open(&directory.join("profile/durable/sage.db")).unwrap();
        let project = database
            .create_project_from_root(&selection.project_root)
            .unwrap();
        let chat = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        let registry = Arc::new(BindingRegistry::desktop_claude_fixture(selection).unwrap());
        let state = AppState {
            database: database.clone(),
            settings: crate::settings_store::SettingsStore::in_memory().unwrap(),
            acp: crate::acp::service::AcpService::new_with_registry_for_test(
                database,
                Arc::clone(&registry),
            )
            .unwrap(),
            harnesses: crate::harnesses::HarnessCatalog::with_registry(registry),
            harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
            editor_operations: crate::editor_coordination::EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(0),
            startup_recovery_notice: None,
            terminals: crate::user_terminal::TerminalSessionPort::desktop(),
        };
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let started = start_acp_session_inner(
            &state,
            StartAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new("bindingId", "claude-agent-acp").unwrap(),
                model_id: None,
            },
        )
        .await
        .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.projection.current_session_id.unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        activate_acp_session_inner(
            &state,
            ActivateAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
            },
        )
        .await
        .unwrap();
        crate::test_wait::until("the fixture's command list", || {
            state
                .acp
                .available_commands(&owner)
                .is_some_and(|list| list.snapshot().revision > 0)
        });
        let list = state.acp.available_commands(&owner).unwrap().snapshot();
        let names: Vec<_> = list.commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["compact", "review", "init"]);
        let revision = list.revision;

        // A stale revision, as after the list changed under a pick, sends
        // nothing: the fixture would refuse any unexpected prompt.
        let stale = run(
            &state,
            command(&state, &owner, revision + 1, "review", Some("42")),
        )
        .await;
        assert!(matches!(
            stale,
            Err(SendAcpTurnFailure {
                prompt_dispatch: PromptDispatch::NotStarted,
                ..
            })
        ));
        assert_eq!(completed_turns(&state, &owner), 0);

        run(
            &state,
            command(&state, &owner, revision, "review", Some("42")),
        )
        .await
        .unwrap();
        crate::test_wait::until("the /review turn", || completed_turns(&state, &owner) == 1);
        run(&state, command(&state, &owner, revision, "compact", None))
            .await
            .unwrap();
        crate::test_wait::until("the /compact turn", || completed_turns(&state, &owner) == 2);
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        send_acp_turn_inner(
            &state,
            SendAcpTurnRequest::Session {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                prompt: Prompt::new("prompt", "Summarize the example").unwrap(),
                attached_context: Default::default(),
            },
        )
        .await
        .unwrap();
        crate::test_wait::until("the plain turn", || completed_turns(&state, &owner) == 3);

        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        close_acp_session_inner(
            &state,
            CloseAcpSessionRequest {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
            },
        )
        .await
        .unwrap();
        crate::test_wait::until("the fixture process to exit", || {
            crate::acp::process::TreeCapacityPool::global()
                .observation_usage()
                .0
                == 0
        });
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        fs::remove_dir_all(directory).unwrap();
    });
}
