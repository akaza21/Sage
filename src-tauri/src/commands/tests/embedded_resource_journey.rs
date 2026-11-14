//! The fixture's `custom-acp-embedded-resources` case answers each prompt
//! with every resource it received (name, type, size and SHA-256), so the
//! transcript proves which bytes reached it.

use sha2::{Digest, Sha256};

use super::*;
use crate::{
    acp::{
        attached_context::files::{PASTED_TEXT_NAME, attachment},
        bindings::{codex::CodexBindingRegistration, registry::BindingRegistry},
        contract::{RuntimeEpoch, SageTurnId, TurnState},
        intent::{
            ActivateAcpSessionRequest, CloseAcpSessionRequest, Prompt, SendAcpTurnRequest,
            StagedSelection,
        },
        scheduler::RuntimeOwner,
    },
    commands::acp::{activate_acp_session_inner, close_acp_session_inner, send_acp_turn_inner},
    settings_store::CustomAcpAgent,
};

const CASE: &str = "custom-acp-embedded-resources";
const AGENT: &str = "custom-acp-1";
const PROMPT: &str = "Summarize the attached log";

fn fixture_agent() -> CustomAcpAgent {
    let executable = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!(
            "acp-adversarial-fixture{}",
            std::env::consts::EXE_SUFFIX
        ));
    assert!(
        executable.is_file(),
        "build the acp-adversarial-fixture binary with --features acp-fixture first"
    );
    CustomAcpAgent {
        id: AGENT.into(),
        name: "Embedded resources fixture".into(),
        executable: executable.to_string_lossy().into_owned(),
        args: ["--case", CASE, "--seed", "0"].map(str::to_owned).to_vec(),
        env: Vec::new(),
    }
}

struct Journey {
    state: AppState,
    owner: RuntimeOwner,
    root: std::path::PathBuf,
}

impl Journey {
    fn page(&self) -> crate::acp::projection::AcpChatPage {
        self.state
            .acp
            .load_chat_page(
                self.owner.project_id.clone(),
                self.owner.chat_id.clone(),
                None,
            )
            .unwrap()
    }

    fn completed_turns(&self) -> usize {
        self.page()
            .turns
            .as_slice()
            .iter()
            .filter(|turn| turn.state == TurnState::Completed)
            .count()
    }

    fn exchange(&self, index: usize) -> (SageTurnId, String, String) {
        let page = self.page();
        let turn = &page.turns.as_slice()[index];
        let text = |wanted: TextRole| {
            turn.items
                .as_slice()
                .iter()
                .find_map(|item| match &item.content {
                    ItemContent::Text { role, text } if *role == wanted => {
                        Some(text.as_str().to_owned())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("turn {index} has no {wanted:?} text"))
        };
        (
            turn.id.clone(),
            text(TextRole::User),
            text(TextRole::Assistant),
        )
    }

    async fn send(&self, staged: StagedSelection) {
        send_acp_turn_inner(
            &self.state,
            SendAcpTurnRequest::Session {
                project_id: self.owner.project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: self.owner.chat_id.clone(),
                expected_projection_revision: self.page().projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: self.owner.session_id.clone(),
                runtime_epoch: self.owner.runtime_epoch,
                prompt: Prompt::new("prompt", PROMPT).unwrap(),
                attached_context: Default::default(),
                staged: Some(staged),
            },
        )
        .await
        .unwrap();
    }
}

async fn open_journey() -> Journey {
    let root = std::env::temp_dir().join(format!("sage-embedded-journey-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let registry = Arc::new(BindingRegistry::with_codex(
        CodexBindingRegistration::unavailable_for_test(),
    ));
    registry.apply_custom_acp_agents(&[fixture_agent()]);
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
        archive_releases: Default::default(),
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
            binding_id: BindingId::new("bindingId", AGENT).unwrap(),
            model_id: None,
            effort: None,
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
            project_id,
            project_generation: ProjectGeneration::ZERO,
            chat_id,
            expected_projection_revision: started.projection.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
        },
    )
    .await
    .unwrap();
    Journey { state, owner, root }
}

#[test]
fn a_staged_paste_reaches_the_agent_as_an_embedded_resource_and_retry_resends_it() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let journey = open_journey().await;
        let (project, chat) = (
            journey.owner.project_id.as_str(),
            journey.owner.chat_id.as_str(),
        );
        // A paste over the composer cap, as the composer stages it.
        let log: String = (0..2000)
            .map(|line| format!("synthetic log line {line:04}\n"))
            .collect();
        let attached_at = attachment(&journey.state.database, project, chat)
            .unwrap()
            .unwrap();
        let revision = journey
            .state
            .acp
            .file_stages
            .lock()
            .unwrap()
            .append_text(project, chat, attached_at, None, log.clone())
            .unwrap()
            .revision;
        let received = format!(
            "Synthetic replay: the prompt carried 1 embedded resource: {PASTED_TEXT_NAME} (text/plain, {} bytes, SHA-256 {:x}).",
            log.len(),
            Sha256::digest(log.as_bytes())
        );

        journey.send(StagedSelection::Revision(revision)).await;
        sage_test_support::test_wait::until("the sent turn", || journey.completed_turns() == 1);
        let (sent, prompt, answer) = journey.exchange(0);
        assert_eq!(prompt, PROMPT);
        assert_eq!(answer, received);
        assert!(
            journey
                .state
                .acp
                .file_stages
                .lock()
                .unwrap()
                .list(project, chat)
                .is_none()
        );

        journey.send(StagedSelection::RetryOf(sent)).await;
        sage_test_support::test_wait::until("the retried turn", || journey.completed_turns() == 2);
        let (_, prompt, answer) = journey.exchange(1);
        assert_eq!(prompt, PROMPT);
        assert_eq!(answer, received);

        close_acp_session_inner(
            &journey.state,
            CloseAcpSessionRequest {
                project_id: journey.owner.project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: journey.owner.chat_id.clone(),
                expected_projection_revision: journey.page().projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: journey.owner.session_id.clone(),
                runtime_epoch: journey.owner.runtime_epoch,
            },
        )
        .await
        .unwrap();
        sage_test_support::test_wait::until("the fixture process to exit", || {
            crate::acp::process::TreeCapacityPool::global()
                .observation_usage()
                .0
                == 0
        });
        journey.state.acp.shutdown_blocking().unwrap();
        let Journey { state, root, .. } = journey;
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}
