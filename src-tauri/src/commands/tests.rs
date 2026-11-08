use std::{
    fs,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use uuid::Uuid;

use super::{
    acp::{
        await_codex_discovery, get_acp_chat_page_inner, get_codex_discovery_inner,
        get_codex_discovery_with_deadline, start_acp_session_inner,
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

#[cfg(windows)]
use super::acp::{
    activate_acp_session_inner, cancel_acp_turn_inner, close_acp_session_inner,
    end_acp_session_inner, send_acp_turn_inner,
};
#[cfg(windows)]
use crate::acp::{
    contract::{EntityRevision, RuntimeEpoch, SageAgentSessionId, SageTurnId, TurnState},
    intent::{
        ActivateAcpSessionRequest, CancelAcpTurnRequest, CloseAcpSessionRequest, Prompt,
        PromptDispatch, SendAcpTurnFailure, SendAcpTurnRequest,
    },
    runtime::{
        HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent, PromptWrite, RuntimeEngine,
        RuntimeHost, RuntimeShutdownRecovery,
    },
    scheduler::RuntimeOwner,
    store::{
        ActivateSessionCommand, AdapterTurnCursor, BeginCancelCommand, CompleteActivationCommand,
        FinalizeCloseCommand, PrepareTurnCommand, SendObservation, StartSessionCommand,
        TurnMutationCommand, activate_session, begin_cancel, begin_send, close_session,
        complete_activation, finalize_close_after_zero_process, prepare_turn,
        record_send_observation,
    },
};
#[cfg(windows)]
use std::sync::Mutex;

#[cfg(windows)]
mod attached_context;

pub(super) fn state_with_project() -> (AppState, String, String, std::path::PathBuf) {
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

fn state_with_unavailable_codex_project() -> (AppState, String, String, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("sage-acp-command-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let codex = crate::acp::bindings::codex::CodexBindingRegistration::unavailable_for_test();
    let acp =
        crate::acp::service::AcpService::new_with_codex_for_test(database.clone(), codex.clone())
            .unwrap();
    let state = AppState {
        database,
        settings: crate::settings_store::SettingsStore::in_memory().unwrap(),
        acp,
        harnesses: crate::harnesses::HarnessCatalog::with_codex_registration(codex),
        harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
        editor_operations: crate::editor_coordination::EditorOperationCoordinator::default(),
        project_transitions: tokio::sync::Mutex::new(0),
        startup_recovery_notice: None,
        terminals: crate::user_terminal::TerminalSessionPort::desktop(),
    };
    (state, project.id, chat_id, root)
}

#[cfg(windows)]
fn state_with_internal_opencode_registry() -> (AppState, String, String, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("sage-opencode-command-{}", Uuid::new_v4()));
    let npm = root.join("npm");
    let shim = npm.join("opencode.cmd");
    let executable = npm
        .join("node_modules")
        .join("opencode-ai")
        .join("bin")
        .join("opencode.exe");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&shim, "@echo off\r\necho 1.18.21\r\n").unwrap();
    fs::write(&executable, b"native-opencode").unwrap();
    fs::write(
        npm.join("node_modules")
            .join("opencode-ai")
            .join("package.json"),
        r#"{"name":"opencode-ai","version":"1.18.21","bin":{"opencode":"./bin/opencode.exe"}}"#,
    )
    .unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let registry = Arc::new(
        crate::acp::bindings::registry::BindingRegistry::new_with_opencode_for_test(
            crate::acp::bindings::codex::CodexBindingRegistration::unavailable_for_test(),
            crate::acp::bindings::claude::ClaudeBindingRegistration::absent_for_test(),
            crate::acp::bindings::standard_acp::OpenCodeBindingRegistration::admitted_for_test(
                &shim, "1.18.21",
            ),
        ),
    );
    let acp = crate::acp::service::AcpService::new_with_registry_for_test(
        database.clone(),
        Arc::clone(&registry),
    )
    .unwrap();
    let state = AppState {
        database,
        settings: crate::settings_store::SettingsStore::in_memory().unwrap(),
        acp,
        harnesses: crate::harnesses::HarnessCatalog::with_registry(registry),
        harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
        editor_operations: crate::editor_coordination::EditorOperationCoordinator::default(),
        project_transitions: tokio::sync::Mutex::new(0),
        startup_recovery_notice: None,
        terminals: crate::user_terminal::TerminalSessionPort::desktop(),
    };
    (state, project.id, chat_id, root)
}

#[cfg(windows)]
async fn seed_internal_opencode_session(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
) -> SageAgentSessionId {
    let context = state
        .lock_active_project(project_id, ProjectGeneration::ZERO)
        .await
        .unwrap();
    let admitted = state
        .acp
        .binding_registry()
        .resolve("opencode-acp")
        .unwrap();
    let project_root = admitted.project_root(&state.database, &context).unwrap();
    drop(context);
    let page = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap();
    state
        .acp
        .start_session(
            admitted,
            &project_root,
            StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new("bindingId", "opencode-acp").unwrap(),
                binding_digest: String::new(),
                harness_version: None,
                model_id: None,
                now_millis: crate::persistence::now_millis(),
            },
        )
        .await
        .unwrap()
        .session_id
}

#[cfg(windows)]
fn state_with_admitted_codex_project() -> (AppState, String, String, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("sage-acp-command-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let codex = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let acp =
        crate::acp::service::AcpService::new_with_codex_for_test(database.clone(), codex.clone())
            .unwrap();
    let state = AppState {
        database,
        settings: crate::settings_store::SettingsStore::in_memory().unwrap(),
        acp,
        harnesses: crate::harnesses::HarnessCatalog::with_codex_registration(codex),
        harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
        editor_operations: crate::editor_coordination::EditorOperationCoordinator::default(),
        project_transitions: tokio::sync::Mutex::new(0),
        startup_recovery_notice: None,
        terminals: crate::user_terminal::TerminalSessionPort::desktop(),
    };
    (state, project.id, chat_id, root)
}

#[cfg(windows)]
fn state_with_registry_test_binding_project() -> (AppState, String, String, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("sage-acp-command-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let codex = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let test_binding = crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
    let registry = Arc::new(
        crate::acp::bindings::registry::BindingRegistry::with_test_binding(codex, test_binding),
    );
    let acp = crate::acp::service::AcpService::new_with_registry_for_test(
        database.clone(),
        Arc::clone(&registry),
    )
    .unwrap();
    let state = AppState {
        database,
        settings: crate::settings_store::SettingsStore::in_memory().unwrap(),
        acp,
        harnesses: crate::harnesses::HarnessCatalog::with_registry(registry),
        harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
        editor_operations: crate::editor_coordination::EditorOperationCoordinator::default(),
        project_transitions: tokio::sync::Mutex::new(0),
        startup_recovery_notice: None,
        terminals: crate::user_terminal::TerminalSessionPort::desktop(),
    };
    (state, project.id, chat_id, root)
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum CommandHostMode {
    Idle,
    Running,
}

#[cfg(windows)]
struct CommandHostEngine {
    database: Database,
    owner: RuntimeOwner,
    calls: Arc<Mutex<Vec<&'static str>>>,
    turn_id: Option<SageTurnId>,
    prompts: Option<Arc<Mutex<Vec<String>>>>,
    fail_dispatch: bool,
}

#[cfg(windows)]
struct CommandShutdownRecovery(u64);

#[cfg(windows)]
impl RuntimeShutdownRecovery for CommandShutdownRecovery {
    fn shutdown_recovery_before(
        &self,
        _: std::time::Instant,
    ) -> Result<crate::acp::process::ZeroProcessToken, HostError> {
        Ok(crate::acp::process::ZeroProcessToken::for_test(self.0))
    }
}

// Database's connection mutex is deliberately non-reentrant. Keep every
// test-engine guard in a lexical block so the command can immediately
// reload the authoritative projection after the mutation returns.

#[cfg(windows)]
impl RuntimeEngine for CommandHostEngine {
    fn activate(&mut self, owner: &RuntimeOwner) -> Result<(), HostError> {
        (owner == &self.owner)
            .then_some(())
            .ok_or(HostError::Intent)
    }

    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        self.calls.lock().unwrap().push("prepare");
        if let Some(prompts) = &self.prompts {
            prompts
                .lock()
                .unwrap()
                .push(intent.command.prompt.as_str().to_owned());
        }
        let prepared = {
            let mut connection = self.database.connection();
            prepare_turn(&mut connection, intent.command.clone()).map_err(|_| HostError::Intent)?
        };
        self.turn_id = Some(prepared.turn_id.clone());
        Ok(prepared.turn_id)
    }

    fn dispatch_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        self.calls.lock().unwrap().push("dispatch");
        if self.fail_dispatch {
            return Err(HostError::RecoveryRequired);
        }
        if owner != &self.owner || self.turn_id.as_ref() != Some(turn_id) {
            return Err(HostError::Intent);
        }
        let sending = {
            let mut connection = self.database.connection();
            begin_send(
                &mut connection,
                TurnMutationCommand {
                    project_id: owner.project_id.clone(),
                    chat_id: owner.chat_id.clone(),
                    session_id: owner.session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    turn_id: turn_id.clone(),
                    expected_revision: EntityRevision::ZERO,
                    now_millis: crate::persistence::now_millis(),
                },
            )
            .map_err(|_| HostError::Intent)?
        };
        {
            let mut connection = self.database.connection();
            record_send_observation(
                &mut connection,
                TurnMutationCommand {
                    project_id: owner.project_id.clone(),
                    chat_id: owner.chat_id.clone(),
                    session_id: owner.session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    turn_id: turn_id.clone(),
                    expected_revision: sending.revision,
                    now_millis: crate::persistence::now_millis(),
                },
                SendObservation::CompleteBytes,
            )
            .map_err(|_| HostError::Intent)?;
        }
        Ok(PromptWrite::Complete(turn_id.clone()))
    }

    fn retry_prompt(&mut self, _: &RuntimeOwner, _: &SageTurnId) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }

    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }

    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }

    fn begin_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        self.calls.lock().unwrap().push("cancel-begin");
        let revision: i64 = self
            .database
            .connection()
            .query_row(
                "SELECT revision FROM turns WHERE id=?1",
                [intent.turn_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|_| HostError::Intent)?;
        {
            let mut connection = self.database.connection();
            begin_cancel(
                &mut connection,
                BeginCancelCommand {
                    turn: TurnMutationCommand {
                        project_id: intent.owner.project_id.clone(),
                        chat_id: intent.owner.chat_id.clone(),
                        session_id: intent.owner.session_id.clone(),
                        runtime_epoch: intent.owner.runtime_epoch,
                        turn_id: intent.turn_id.clone(),
                        expected_revision: EntityRevision::new(revision as u64)
                            .map_err(|_| HostError::Intent)?,
                        now_millis: intent.now_millis,
                    },
                    client_intent_id: intent.client_intent_id.clone(),
                },
            )
            .map_err(|_| HostError::Intent)?;
        }
        Ok(())
    }

    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        self.calls.lock().unwrap().push("cancel-dispatch");
        Ok(())
    }

    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Ok(())
    }

    fn begin_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError> {
        self.calls.lock().unwrap().push("close-begin");
        {
            let mut connection = self.database.connection();
            close_session(&mut connection, intent.command.clone())
                .map_err(|_| HostError::Intent)?;
        }
        Ok(())
    }

    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        self.calls.lock().unwrap().push("close-zero");
        Ok(())
    }

    fn finalize_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError> {
        self.calls.lock().unwrap().push("close-finalize");
        {
            let mut connection = self.database.connection();
            finalize_close_after_zero_process(
                &mut connection,
                FinalizeCloseCommand {
                    request: intent.command.clone(),
                },
            )
            .map_err(|_| HostError::Intent)?;
        }
        Ok(())
    }

    fn shutdown(&mut self, _: std::time::Instant) -> Result<(), HostError> {
        self.calls.lock().unwrap().push("shutdown");
        Ok(())
    }
}

#[cfg(windows)]
fn install_command_host(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    session_id: &SageAgentSessionId,
    mode: CommandHostMode,
    close_unproven: bool,
) -> (RuntimeOwner, Arc<Mutex<Vec<&'static str>>>) {
    let binding_id: String = state
        .database
        .connection()
        .query_row(
            "SELECT binding_id FROM agent_sessions WHERE id=?1",
            [session_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    let admitted = state.acp.binding_registry().resolve(&binding_id).unwrap();
    let binding = admitted.identity();
    let owner = RuntimeOwner {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    let activation_revision = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    let activated = {
        let mut connection = state.database.connection();
        activate_session(
            &mut connection,
            ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: activation_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                artifact_digest: binding.artifact_digest.to_owned(),
                now_millis: crate::persistence::now_millis(),
            },
        )
        .unwrap()
    };
    let mut capabilities = admitted.supported_capabilities().unwrap();
    if close_unproven {
        capabilities.close_session = false;
    }
    {
        let mut connection = state.database.connection();
        complete_activation(
            &mut connection,
            CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                binding_digest: binding.binding_digest_hex.to_owned(),
                artifact_digest: binding.artifact_digest.to_owned(),
                external_session_id: crate::acp::intent::ExternalId::new(
                    "externalSessionId",
                    "thread-command-route",
                )
                .unwrap(),
                capabilities_json: serde_json::to_string(&capabilities).unwrap(),
                now_millis: crate::persistence::now_millis(),
            },
        )
        .unwrap();
    }
    let calls = Arc::new(Mutex::new(Vec::new()));
    let host = Arc::new(
        RuntimeHost::start_with_shutdown_recovery(
            CommandHostEngine {
                database: state.database.clone(),
                owner: owner.clone(),
                calls: Arc::clone(&calls),
                turn_id: None,
                prompts: None,
                fail_dispatch: false,
            },
            Arc::new(CommandShutdownRecovery(owner.runtime_epoch.get())),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    if matches!(mode, CommandHostMode::Running) {
        let revision = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        host.send(HostPromptIntent {
            command: PrepareTurnCommand {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::parse("intent-command-seed").unwrap(),
                prompt: Prompt::new("prompt", "seed turn").unwrap(),
                now_millis: crate::persistence::now_millis(),
            },
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
        calls.lock().unwrap().clear();
    }
    state.acp.install_runtime_for_test(owner.clone(), host);
    assert_eq!(activated.runtime_epoch, Some(owner.runtime_epoch));
    (owner, calls)
}

#[cfg(windows)]
async fn start_admitted_command_session(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
) -> SageAgentSessionId {
    let context = state
        .lock_active_project(project_id, ProjectGeneration::ZERO)
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
        .seed_session_validation_for_test(&authority, binding.binding_digest(), None);
    let page = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap();
    start_acp_session_inner(
        state,
        StartAcpSessionRequest {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
            model_id: None,
        },
    )
    .await
    .unwrap()
    .projection
    .current_session_id
    .unwrap()
}

#[cfg(windows)]
async fn start_registry_test_command_session(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
) -> SageAgentSessionId {
    let context = state
        .lock_active_project(project_id, ProjectGeneration::ZERO)
        .await
        .unwrap();
    let authority = state
        .database
        .codex_project_root_authority(&context)
        .unwrap();
    drop(context);
    let binding = state
        .acp
        .binding_registry()
        .test_registration()
        .unwrap()
        .refresh()
        .unwrap();
    state
        .acp
        .codex_discovery_manager()
        .seed_session_validation_for_binding_for_test(
            &authority,
            binding.binding_digest(),
            crate::acp::bindings::registry::TEST_BINDING_ID,
            None,
        );
    let page = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap();
    start_acp_session_inner(
        state,
        StartAcpSessionRequest {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new(
                "bindingId",
                crate::acp::bindings::registry::TEST_BINDING_ID,
            )
            .unwrap(),
            model_id: None,
        },
    )
    .await
    .unwrap()
    .projection
    .current_session_id
    .unwrap()
}

#[test]
fn dark_codex_discovery_is_typed_and_does_not_mutate_sqlite() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_unavailable_codex_project();
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

        // A discovered executable is not an ACP binding. With no independently
        // admitted binding, discovery must remain typed and fail closed.
        let expected_reason = AcpUnavailableReason::BindingUnavailable;
        assert!(
            matches!(
                result,
                Err(AcpIpcError::Unavailable { reason, .. }) if reason == expected_reason
            ),
            "{result:?}"
        );
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

#[cfg(windows)]
#[test]
fn legacy_access_migration_requires_a_restored_session_not_a_catalog_default() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (mut state, project, chat, root) = state_with_admitted_codex_project();
        state.harnesses = state.harnesses.clone().settled_for_test();
        let settings_path = root.join("legacy-settings.sqlite");
        state.settings = crate::settings_store::SettingsStore::open(&settings_path).unwrap();
        rusqlite::Connection::open(&settings_path)
            .unwrap()
            .execute(
                "INSERT INTO session_access_mode VALUES (1,'workspaceWrite')",
                [],
            )
            .unwrap();
        let snapshot = state
            .database
            .bootstrap_snapshot_with_catalog(&state.harnesses)
            .unwrap();
        assert!(snapshot.session_id.is_none());
        assert_eq!(snapshot.selected_harness_id.as_deref(), Some("codex"));
        assert!(
            crate::settings_commands::access_preferences(&state)
                .unwrap()
                .is_empty()
        );
        assert!(!state.settings.pending_access_migration().unwrap());
        start_admitted_command_session(
            &state,
            &SageProjectId::parse(project).unwrap(),
            &SageChatId::parse(chat).unwrap(),
        )
        .await;
        assert!(
            crate::settings_commands::access_preferences(&state)
                .unwrap()
                .is_empty()
        );
        rusqlite::Connection::open(&settings_path)
            .unwrap()
            .execute(
                "INSERT INTO session_access_mode VALUES (1,'workspaceWrite')",
                [],
            )
            .unwrap();
        assert!(state.settings.session_access_modes().is_err());
        let modes = crate::settings_commands::access_preferences(&state).unwrap();
        assert_eq!(modes.len(), 1);
        assert_eq!(
            modes["codex-app-server"],
            crate::settings_store::SessionAccessMode::WorkspaceWrite
        );
        assert!(!state.settings.pending_access_migration().unwrap());
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn codex_discovery_deadline_bounds_the_project_transition_wait() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, _chat, root) = state_with_unavailable_codex_project();
        let project = SageProjectId::parse(project).unwrap();
        let held_transition = state.project_transitions.lock().await;
        let request = GetCodexDiscoveryRequest {
            project_id: project.clone(),
            project_generation: ProjectGeneration::ZERO,
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
        };
        // The test state deliberately has no independently admitted binding.
        let expected_reason = AcpUnavailableReason::BindingUnavailable;
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
            Err(AcpIpcError::Unavailable { reason, .. }) if reason == expected_reason
        ));
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn codex_discovery_reports_admission_state_while_a_live_session_holds_the_runtime() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, _chat, root) = state_with_unavailable_codex_project();
        let project = SageProjectId::parse(project).unwrap();
        let live = state.acp.hold_live_operation_for_test();
        let request = GetCodexDiscoveryRequest {
            project_id: project,
            project_generation: ProjectGeneration::ZERO,
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
        };

        let result = get_codex_discovery_inner(&state, request).await;

        assert!(
            !matches!(result, Err(AcpIpcError::Capacity { .. })),
            "{result:?}"
        );
        let expected_reason = AcpUnavailableReason::BindingUnavailable;
        assert!(
            matches!(
                result,
                Err(AcpIpcError::Unavailable { reason, .. }) if reason == expected_reason
            ),
            "{result:?}"
        );
        drop(live);
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
async fn seed_codex_discovery_catalog(
    state: &AppState,
    project: &SageProjectId,
    model: crate::acp::intent::ModelId,
) {
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
        .seed_session_validation_for_test(&authority, binding.binding_digest(), Some(model));
}

#[cfg(windows)]
#[test]
fn codex_discovery_serves_the_admitted_catalog_while_a_live_session_holds_the_runtime() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, _chat, root) = state_with_admitted_codex_project();
        let project = SageProjectId::parse(project).unwrap();
        let model = crate::acp::intent::ModelId::new("modelId", "gpt-5-codex").unwrap();
        seed_codex_discovery_catalog(&state, &project, model.clone()).await;
        let live = state.acp.hold_live_operation_for_test();
        let request = GetCodexDiscoveryRequest {
            project_id: project.clone(),
            project_generation: ProjectGeneration::ZERO,
            binding_id: BindingId::new("bindingId", crate::acp::bindings::codex::CODEX_BINDING_ID)
                .unwrap(),
        };

        let result = get_codex_discovery_inner(&state, request).await;

        let response = result.expect("an admitted cached catalog is served under a live session");
        assert_eq!(response.project_id, project);
        assert_eq!(
            response
                .models
                .iter()
                .map(|entry| entry.id.clone())
                .collect::<Vec<_>>(),
            vec![model]
        );
        assert!(matches!(
            response.account,
            crate::acp::intent::CodexDiscoveryAccount::ManagedChatgpt
        ));
        drop(live);
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn codex_discovery_refuses_an_uncached_catalog_only_when_live_sessions_fill_every_lane() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, _chat, root) = state_with_admitted_codex_project();
        let project = SageProjectId::parse(project).unwrap();
        let request = || GetCodexDiscoveryRequest {
            project_id: project.clone(),
            project_generation: ProjectGeneration::ZERO,
            binding_id: BindingId::new("bindingId", crate::acp::bindings::codex::CODEX_BINDING_ID)
                .unwrap(),
        };
        let is_capacity_refusal = |result: &Result<_, AcpIpcError>| {
            matches!(
                result,
                Err(AcpIpcError::Capacity {
                    resource: crate::acp::intent::AcpCapacityResource::Queue,
                    limit: 1,
                    ..
                })
            )
        };
        let mut live: Vec<_> = (0..crate::acp::service::LIVE_RUNTIME_LANES)
            .map(|_| state.acp.hold_live_operation_for_test())
            .collect();
        let started = std::time::Instant::now();

        let full = get_codex_discovery_inner(&state, request()).await;

        assert!(is_capacity_refusal(&full), "{full:?}");
        assert!(
            started.elapsed()
                < std::time::Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.spawn),
            "a cache miss with every lane live must refuse without launching a runtime"
        );

        // One live session (Claude, Pi, or a retained completed one) leaves
        // spare capacity, so discovery is admitted rather than refused.
        live.truncate(1);
        let spare = get_codex_discovery_inner(&state, request()).await;
        assert!(!is_capacity_refusal(&spare), "{spare:?}");
        drop(live);
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn codex_discovery_cleanup_deadline_requests_teardown_without_waiting_for_worker_exit() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let timed_out = Arc::new(AtomicBool::new(false));
        let timeout_signal = Arc::clone(&timed_out);
        let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let worker_gate = Arc::clone(&gate);
        let task = tauri::async_runtime::spawn_blocking(move || {
            let (released, ready) = &*worker_gate;
            let mut released = released.lock().unwrap();
            while !*released {
                let (next, timed) = ready
                    .wait_timeout(released, crate::test_wait::HANG_GUARD)
                    .unwrap();
                released = next;
                assert!(
                    !timed.timed_out() || *released,
                    "the codex discovery worker was never released by its test"
                );
            }
            Err(crate::acp::bindings::codex::CodexDiscoveryRunError::Timeout)
        });

        let result = await_codex_discovery(
            task,
            CodexDiscoveryDeadline::for_test(
                std::time::Duration::from_millis(1),
                std::time::Duration::from_millis(10),
            ),
            move || timeout_signal.store(true, Ordering::Release),
        )
        .await;

        let (released, ready) = &*gate;
        *released.lock().unwrap() = true;
        ready.notify_all();

        assert!(timed_out.load(Ordering::Acquire));
        assert!(matches!(
            result,
            Err(AcpIpcError::Unavailable {
                reason: AcpUnavailableReason::CapabilityUnavailable,
                ..
            })
        ));
    });
}

#[test]
fn page_read_uses_native_generation_and_projects_v5_history() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_project();
        let history = state
            .database
            .append_simulator_turn(&chat, "page projection prompt", "page projection answer")
            .unwrap();
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
        assert_eq!(
            page.turns.as_slice()[0].session_id.as_str(),
            history.session_id
        );
        assert_eq!(page.turns.as_slice()[0].items.as_slice().len(), 2);
        assert!(matches!(
            &page.turns.as_slice()[0].items.as_slice()[0].content,
            ItemContent::Text {
                role: TextRole::User,
                text
            } if text.as_str() == "page projection prompt"
        ));
        assert!(matches!(
            &page.turns.as_slice()[0].items.as_slice()[1].content,
            ItemContent::Text {
                role: TextRole::Assistant,
                text
            } if text.as_str() == "page projection answer"
        ));
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn start_preserves_project_root_failure_classification() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let original_root: (String, String) = state
            .database
            .connection()
            .query_row(
                "SELECT id,absolute_path FROM project_roots WHERE project_id=?1",
                [project_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        state
            .database
            .connection()
            .execute(
                "INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal)
                 VALUES(?1,?2,?3,?4,1)",
                rusqlite::params![
                    "root-extra",
                    project_id.as_str(),
                    original_root.1,
                    "test-extra-canonical-key",
                ],
            )
            .unwrap();
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();

        let result = start_acp_session_inner(
            &state,
            StartAcpSessionRequest {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
                model_id: None,
            },
        )
        .await;

        assert!(matches!(
            result,
            Err(AcpIpcError::Unavailable { reason, message })
                if reason == AcpUnavailableReason::CapabilityUnavailable
                    && message.as_str() == "Codex beta requires one unchanged Project root."
        ));
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(all(windows, feature = "acp-fixture"))]
#[test]
fn characterization_new_session_send_preserves_invalid_request_classification() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        use crate::acp::service::{
            CharacterizationArtifact, CharacterizationBinding, CharacterizationCase,
        };

        let root =
            std::env::temp_dir().join(format!("sage-acp-characterization-{}", Uuid::new_v4()));
        let project_root = root.join("project");
        fs::create_dir_all(&project_root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&project_root).unwrap();
        let chat = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        let artifact_path = std::env::current_exe().unwrap();
        let metadata = std::fs::metadata(&artifact_path).unwrap();
        let bytes = std::fs::read(&artifact_path).unwrap();
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(bytes);
        let artifact =
            CharacterizationArtifact::new(artifact_path, hasher.finalize().into(), metadata.len())
                .unwrap();
        let state = AppState::try_new_characterization(
            database,
            CharacterizationBinding::new(artifact, CharacterizationCase::TurnSuccess, &root)
                .unwrap(),
        )
        .unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();

        let result = send_acp_turn_inner(
            &state,
            SendAcpTurnRequest::NewSession {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new(
                    "bindingId",
                    crate::acp::service::CHARACTERIZATION_BINDING_ID,
                )
                .unwrap(),
                model_id: None,
                prompt: Prompt::new("prompt", "fixture new session").unwrap(),
                attached_context: Default::default(),
            },
        )
        .await;

        assert!(matches!(
            result,
            Err(SendAcpTurnFailure {
                failure: AcpIpcError::InvalidRequest { message },
                prompt_dispatch: PromptDispatch::NotStarted,
            }) if message.as_str() == "Start and activate the characterized session before sending."
        ));
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn unknown_requested_binding_is_rejected_before_any_durable_mutation() {
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

#[cfg(windows)]
#[test]
fn an_admitted_opencode_binding_starts_a_durable_session() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_internal_opencode_registry();
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
                binding_id: BindingId::new("bindingId", "opencode-acp").unwrap(),
                model_id: None,
            },
        )
        .await
        .unwrap();
        assert!(result.projection.current_session_id.is_some());
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
        assert_eq!(after.0, before.0 + 1);
        assert!(after.2 > before.2);
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn an_admitted_opencode_session_routes_the_mandatory_core_through_its_own_binding() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_internal_opencode_registry();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = seed_internal_opencode_session(&state, &project_id, &chat_id).await;
        let binding_id = BindingId::new("bindingId", "opencode-acp").unwrap();
        assert!(state.acp.resolve_session_binding(&binding_id).is_ok());
        assert!(state.acp.binding_registry().resolve("opencode-acp").is_ok());
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let runtime_epoch = RuntimeEpoch::new(1).unwrap();

        let activation = activate_acp_session_inner(
            &state,
            ActivateAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch,
            },
        )
        .await;
        assert!(
            activation.is_err(),
            "the fixture OpenCode artifact must fail closed at launch"
        );

        let turn = state.acp.dispatch_session_turn(
            &binding_id,
            HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id: project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    runtime_epoch,
                    expected_projection_revision: page.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    prompt: Prompt::new("prompt", "must not dispatch").unwrap(),
                    now_millis: crate::persistence::now_millis(),
                },
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            },
        );
        assert!(matches!(
            turn,
            Err(crate::acp::service::PromptSendError { error, .. })
                if !matches!(error, crate::acp::service::SessionDispatchError::Binding)
        ));

        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch,
        };
        assert!(matches!(
            state.acp.dispatch_session_cancel(
                &binding_id,
                HostCancelIntent {
                    owner,
                turn_id: SageTurnId::parse("turn-opencode-dormant").unwrap(),
                client_intent_id: ClientIntentId::new(),
                now_millis: crate::persistence::now_millis(),
                },
            ),
            Err(reason) if !matches!(reason, crate::acp::service::SessionDispatchError::Binding)
        ));
        assert!(matches!(
            state.acp.dispatch_session_close(
                &binding_id,
                crate::acp::store::CloseSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: page.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    session_id,
                    runtime_epoch,
                    now_millis: crate::persistence::now_millis(),
                },
            ),
            Err(reason) if !matches!(reason, crate::acp::service::SessionDispatchError::Binding)
        ));
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn unavailable_registration_rejects_the_codex_name_without_routing_by_name() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_unavailable_codex_project();
        let project = SageProjectId::parse(project).unwrap();
        let chat = SageChatId::parse(chat).unwrap();
        let page = state
            .acp
            .load_chat_page(project.clone(), chat.clone(), None)
            .unwrap();
        fs::remove_dir_all(&root).unwrap();

        let result = start_acp_session_inner(
            &state,
            StartAcpSessionRequest {
                project_id: project.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
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
        let after = state
            .acp
            .load_chat_page(project.clone(), chat, None)
            .unwrap();
        assert_eq!(after, page);
        drop(state);
    });
}

#[cfg(windows)]
#[test]
fn admitted_registration_rejects_a_foreign_binding_name() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project = SageProjectId::parse(project).unwrap();
        let chat = SageChatId::parse(chat).unwrap();
        let page = state
            .acp
            .load_chat_page(project.clone(), chat.clone(), None)
            .unwrap();

        let result = start_acp_session_inner(
            &state,
            StartAcpSessionRequest {
                project_id: project.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new("bindingId", "foreign-app-server").unwrap(),
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
        let after = state.acp.load_chat_page(project, chat, None).unwrap();
        assert_eq!(after, page);
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn test_binding_routes_through_registry_resolution_before_durable_start() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_registry_test_binding_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        let authority = state
            .database
            .codex_project_root_authority(&context)
            .unwrap();
        drop(context);
        let binding = state
            .acp
            .binding_registry()
            .test_registration()
            .unwrap()
            .refresh()
            .unwrap();
        state
            .acp
            .codex_discovery_manager()
            .seed_session_validation_for_binding_for_test(
                &authority,
                binding.binding_digest(),
                crate::acp::bindings::registry::TEST_BINDING_ID,
                None,
            );
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();

        let result = start_acp_session_inner(
            &state,
            StartAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new(
                    "bindingId",
                    crate::acp::bindings::registry::TEST_BINDING_ID,
                )
                .unwrap(),
                model_id: None,
            },
        )
        .await
        .unwrap();

        let durable: (String, String) = state
            .database
            .connection()
            .query_row(
                "SELECT binding_id,binding_digest FROM agent_sessions WHERE id=?1",
                [result.projection.current_session_id.unwrap().as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(durable.0, crate::acp::bindings::registry::TEST_BINDING_ID);
        assert_eq!(durable.1, binding.binding_digest_hex());
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn admitted_codex_start_command_routes_to_the_durable_service() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
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
            .seed_session_validation_for_test(&authority, binding.binding_digest(), None);
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();

        let result = start_acp_session_inner(
            &state,
            StartAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
                model_id: None,
            },
        )
        .await
        .unwrap();

        let session_id = result.projection.current_session_id.unwrap();
        let durable: (String, String, String) = state
            .database
            .connection()
            .query_row(
                "SELECT binding_id,binding_digest,state FROM agent_sessions WHERE id=?1",
                [session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(durable.0, "codex-app-server");
        assert_eq!(durable.1, binding.binding_digest_hex());
        assert_eq!(durable.2, "created");
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn test_binding_activation_uses_the_registry_factory_path() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_registry_test_binding_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        let authority = state
            .database
            .codex_project_root_authority(&context)
            .unwrap();
        drop(context);
        let binding = state
            .acp
            .binding_registry()
            .test_registration()
            .unwrap()
            .refresh()
            .unwrap();
        state
            .acp
            .codex_discovery_manager()
            .seed_session_validation_for_binding_for_test(
                &authority,
                binding.binding_digest(),
                crate::acp::bindings::registry::TEST_BINDING_ID,
                None,
            );
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
                binding_id: BindingId::new(
                    "bindingId",
                    crate::acp::bindings::registry::TEST_BINDING_ID,
                )
                .unwrap(),
                model_id: None,
            },
        )
        .await
        .unwrap();
        assert!(
            !crate::acp::bindings::registry::take_test_factory_construction_for_test(),
            "start must not construct a runtime"
        );
        let session_id = started.projection.current_session_id.clone().unwrap();
        let activation = activate_acp_session_inner(
            &state,
            ActivateAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            },
        )
        .await;
        assert!(activation.is_err(), "test fixture launch must fail closed");
        assert!(
            crate::acp::bindings::registry::take_test_factory_construction_for_test(),
            "activation must construct through the test record's registry factory"
        );
        let dispatch = state.acp.dispatch_session_turn(
            &BindingId::new("bindingId", crate::acp::bindings::registry::TEST_BINDING_ID).unwrap(),
            HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id,
                    project_generation: ProjectGeneration::ZERO,
                    chat_id,
                    session_id,
                    runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                    expected_projection_revision: started.projection.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    prompt: Prompt::new("prompt", "test dispatch").unwrap(),
                    now_millis: crate::persistence::now_millis(),
                },
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            },
        );
        assert!(dispatch.is_err(), "test dispatch must fail closed");
        assert!(
            crate::acp::bindings::registry::take_test_turn_dispatch_for_test(),
            "send must dispatch through the test record rather than the shared live operation"
        );
        let cancel = state.acp.dispatch_session_cancel(
            &BindingId::new("bindingId", crate::acp::bindings::registry::TEST_BINDING_ID).unwrap(),
            HostCancelIntent {
                owner: RuntimeOwner {
                    project_id: SageProjectId::parse("project-dispatch").unwrap(),
                    chat_id: SageChatId::parse("chat-dispatch").unwrap(),
                    session_id: SageAgentSessionId::parse("session-dispatch").unwrap(),
                    runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                },
                turn_id: SageTurnId::parse("turn-dispatch").unwrap(),
                client_intent_id: ClientIntentId::new(),
                now_millis: crate::persistence::now_millis(),
            },
        );
        assert!(cancel.is_err(), "test cancel dispatch must fail closed");
        assert!(
            crate::acp::bindings::registry::take_test_cancel_dispatch_for_test(),
            "cancel must route through the test record without re-admission"
        );
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn test_binding_session_operations_dispatch_by_durable_binding_id() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_registry_test_binding_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = start_registry_test_command_session(&state, &project_id, &chat_id).await;
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        };
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let activated = {
            let mut connection = state.database.connection();
            activate_session(
                &mut connection,
                ActivateSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: page.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    session_id: session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    artifact_digest: "11".repeat(32),
                    now_millis: crate::persistence::now_millis(),
                },
            )
            .unwrap()
        };
        assert_eq!(activated.runtime_epoch, Some(owner.runtime_epoch));
        let host = Arc::new(
            RuntimeHost::start(CommandHostEngine {
                database: state.database.clone(),
                owner: owner.clone(),
                calls: Arc::new(Mutex::new(Vec::new())),
                turn_id: None,
                prompts: None,
                fail_dispatch: false,
            })
            .unwrap(),
        );
        host.activate(owner.clone()).unwrap();
        state.acp.install_runtime_for_test(owner.clone(), host);
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();

        let send = send_acp_turn_inner(
            &state,
            SendAcpTurnRequest::Session {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                prompt: Prompt::new("prompt", "dispatch by durable binding").unwrap(),
                attached_context: Default::default(),
                staged: None,
            },
        )
        .await;
        assert!(send.is_err(), "test turn dispatch must fail closed");
        assert!(
            crate::acp::bindings::registry::take_test_turn_dispatch_for_test(),
            "send must select the test record from the durable binding ID"
        );

        let current = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        state
            .acp
            .binding_registry()
            .test_registration()
            .unwrap()
            .induce_drift_for_test();
        let close = close_acp_session_inner(
            &state,
            CloseAcpSessionRequest {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: current.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id,
                runtime_epoch: owner.runtime_epoch,
            },
        )
        .await;
        assert!(close.is_err(), "test close dispatch must fail closed");
        assert!(
            crate::acp::bindings::registry::take_test_close_dispatch_for_test(),
            "close must select the test record from the durable binding ID"
        );
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn admitted_codex_activate_command_reaches_launch_gate_and_rolls_back_exact_owner() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = start_admitted_command_session(&state, &project_id, &chat_id).await;
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let before_processes = crate::acp::process::TreeCapacityPool::global().usage();

        let result = activate_acp_session_inner(
            &state,
            ActivateAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            },
        )
        .await;

        assert!(matches!(
            result,
            Err(AcpIpcError::Unavailable {
                reason: AcpUnavailableReason::CapabilityUnavailable,
                ..
            })
        ));
        let durable: (String, String, String) = state
            .database
            .connection()
            .query_row(
                "SELECT s.state,s.residency,e.state FROM agent_sessions s
                     JOIN session_runtime_epochs e ON e.session_id=s.id
                     WHERE s.id=?1 AND e.runtime_epoch=1",
                [session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(durable, ("created".into(), "cold".into(), "aborted".into()));
        assert_eq!(
            crate::acp::process::TreeCapacityPool::global().usage(),
            before_processes
        );

        let current = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let foreign = activate_acp_session_inner(
            &state,
            ActivateAcpSessionRequest {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: current.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: SageAgentSessionId::parse("session-foreign-route").unwrap(),
                runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            },
        )
        .await;
        assert!(matches!(
            foreign,
            Err(AcpIpcError::Conflict {
                resource: AcpConflictResource::Session,
                ..
            })
        ));
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn admitted_codex_send_command_routes_only_to_the_exact_owned_host() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = start_admitted_command_session(&state, &project_id, &chat_id).await;
        let (owner, calls) = install_command_host(
            &state,
            &project_id,
            &chat_id,
            &session_id,
            CommandHostMode::Idle,
            false,
        );
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let response = send_acp_turn_inner(
            &state,
            SendAcpTurnRequest::Session {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                prompt: Prompt::new("prompt", "route this prompt").unwrap(),
                attached_context: Default::default(),
                staged: None,
            },
        )
        .await
        .unwrap();
        let turn = response
            .projection
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.session_id == session_id)
            .unwrap();
        assert_eq!(turn.state, TurnState::Sending);
        assert_eq!(*calls.lock().unwrap(), ["prepare", "dispatch"]);

        let stale = send_acp_turn_inner(
            &state,
            SendAcpTurnRequest::Session {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: response.projection.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id,
                runtime_epoch: RuntimeEpoch::new(2).unwrap(),
                prompt: Prompt::new("prompt", "must reject").unwrap(),
                attached_context: Default::default(),
                staged: None,
            },
        )
        .await;
        assert!(matches!(
            stale,
            Err(SendAcpTurnFailure {
                failure: AcpIpcError::Conflict {
                    resource: AcpConflictResource::RuntimeEpoch,
                    ..
                },
                prompt_dispatch: PromptDispatch::NotStarted,
            })
        ));
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn admitted_claude_default_access_sends_without_requiring_a_preference_change() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        use crate::harness_discovery::{ClaudeLaunchDiscovery, DiscoveredClaudeLaunch};
        let (mut state, project, chat, root) = state_with_admitted_codex_project();
        state
            .settings
            .set_session_access_modes(&std::collections::BTreeMap::from([(
                "codex-app-server".into(),
                crate::settings_store::SessionAccessMode::WorkspaceWrite,
            )]))
            .unwrap();
        let adapter = root.join("adapter.js");
        let node = root.join("node.exe");
        fs::write(&adapter, b"adapter").unwrap();
        fs::write(&node, b"node").unwrap();
        let claude = crate::acp::bindings::claude::ClaudeBindingRegistration::admitted_for_test(
            ClaudeLaunchDiscovery::Present(Box::new(DiscoveredClaudeLaunch {
                adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
                node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
                adapter_path: adapter,
                adapter_version: "0.76.0".into(),
                acp_sdk_version: "9.9.9".into(),
                node_engine: ">=22".into(),
                node_path: node,
                node_version: "22.18.0".into(),
            })),
        );
        let registry = Arc::new(crate::acp::bindings::registry::BindingRegistry::new(
            crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test(),
            claude,
        ));
        assert!(registry.claude().bound_build().is_some());
        state.acp = crate::acp::service::AcpService::new_with_registry_for_test(
            state.database.clone(),
            Arc::clone(&registry),
        )
        .unwrap();
        state.harnesses = crate::harnesses::HarnessCatalog::with_registry(registry);
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        let admitted = state
            .acp
            .binding_registry()
            .resolve("claude-agent-acp")
            .unwrap();
        let project_root = admitted.project_root(&state.database, &context).unwrap();
        drop(context);
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let session_id = state
            .acp
            .start_session(
                admitted,
                &project_root,
                StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: page.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: BindingId::new("bindingId", "claude-agent-acp").unwrap(),
                    binding_digest: String::new(),
                    harness_version: None,
                    model_id: None,
                    now_millis: crate::persistence::now_millis(),
                },
            )
            .await
            .unwrap()
            .session_id;
        let (owner, calls) = install_command_host(
            &state,
            &project_id,
            &chat_id,
            &session_id,
            CommandHostMode::Idle,
            false,
        );
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let result = send_acp_turn_inner(
            &state,
            SendAcpTurnRequest::Session {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id,
                runtime_epoch: owner.runtime_epoch,
                prompt: Prompt::new("prompt", "must not run").unwrap(),
                attached_context: Default::default(),
                staged: None,
            },
        )
        .await;
        assert!(
            result.is_ok(),
            "Claude default must permit sending: {result:?}"
        );
        assert_eq!(*calls.lock().unwrap(), ["prepare", "dispatch"]);
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn admitted_codex_cancel_command_routes_only_the_current_turn_owner() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = start_admitted_command_session(&state, &project_id, &chat_id).await;
        let (owner, calls) = install_command_host(
            &state,
            &project_id,
            &chat_id,
            &session_id,
            CommandHostMode::Running,
            false,
        );
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let turn = page
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.session_id == session_id)
            .unwrap();
        let turn_id = turn.id.clone();
        let turn_state = turn.state;
        let turn_revision = turn.revision;
        let result = cancel_acp_turn_inner(
            &state,
            CancelAcpTurnRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                turn_id: turn_id.clone(),
                expected_turn_state: turn_state,
                expected_turn_revision: turn_revision,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            result
                .projection
                .turns
                .as_slice()
                .iter()
                .find(|turn| turn.id == turn_id)
                .unwrap()
                .state,
            TurnState::Cancelling
        );
        assert_eq!(*calls.lock().unwrap(), ["cancel-begin", "cancel-dispatch"]);

        let stale = cancel_acp_turn_inner(
            &state,
            CancelAcpTurnRequest {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: result.projection.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id,
                runtime_epoch: owner.runtime_epoch,
                turn_id: SageTurnId::parse("turn-foreign-route").unwrap(),
                expected_turn_state: TurnState::Cancelling,
                expected_turn_revision: EntityRevision::new(turn_revision.get() + 1).unwrap(),
            },
        )
        .await;
        assert!(matches!(
            stale,
            Err(AcpIpcError::Conflict {
                resource: AcpConflictResource::TurnState,
                ..
            })
        ));
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn admitted_codex_cancel_command_accepts_a_turn_advanced_by_ingress() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = start_admitted_command_session(&state, &project_id, &chat_id).await;
        let (owner, calls) = install_command_host(
            &state,
            &project_id,
            &chat_id,
            &session_id,
            CommandHostMode::Idle,
            false,
        );
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let sent = send_acp_turn_inner(
            &state,
            SendAcpTurnRequest::Session {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                prompt: Prompt::new("prompt", "cancel after ingress").unwrap(),
                attached_context: Default::default(),
                staged: None,
            },
        )
        .await
        .unwrap();
        let turn = sent
            .projection
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.session_id == session_id)
            .unwrap()
            .clone();
        assert_eq!(turn.state, TurnState::Sending);
        {
            let mut connection = state.database.connection();
            record_send_observation(
                &mut connection,
                TurnMutationCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    runtime_epoch: owner.runtime_epoch,
                    turn_id: turn.id.clone(),
                    expected_revision: turn.revision,
                    now_millis: crate::persistence::now_millis(),
                },
                SendObservation::Correlated,
            )
            .unwrap();
        }
        calls.lock().unwrap().clear();

        let result = cancel_acp_turn_inner(
            &state,
            CancelAcpTurnRequest {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: sent.projection.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id,
                runtime_epoch: owner.runtime_epoch,
                turn_id: turn.id.clone(),
                expected_turn_state: turn.state,
                expected_turn_revision: turn.revision,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            result
                .projection
                .turns
                .as_slice()
                .iter()
                .find(|candidate| candidate.id == turn.id)
                .unwrap()
                .state,
            TurnState::Cancelling
        );
        assert_eq!(*calls.lock().unwrap(), ["cancel-begin", "cancel-dispatch"]);
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn admitted_codex_close_command_routes_and_rejects_a_foreign_session() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = start_admitted_command_session(&state, &project_id, &chat_id).await;
        let (owner, calls) = install_command_host(
            &state,
            &project_id,
            &chat_id,
            &session_id,
            CommandHostMode::Idle,
            false,
        );
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let foreign = close_acp_session_inner(
            &state,
            CloseAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: SageAgentSessionId::parse("session-foreign-route").unwrap(),
                runtime_epoch: owner.runtime_epoch,
            },
        )
        .await;
        assert!(matches!(
            foreign,
            Err(AcpIpcError::Conflict {
                resource: AcpConflictResource::Session,
                ..
            })
        ));

        let result = close_acp_session_inner(
            &state,
            CloseAcpSessionRequest {
                project_id,
                project_generation: ProjectGeneration::ZERO,
                chat_id,
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id,
                runtime_epoch: owner.runtime_epoch,
            },
        )
        .await
        .unwrap();
        assert!(result.projection.current_session_id.is_none());
        assert_eq!(
            *calls.lock().unwrap(),
            ["close-begin", "close-zero", "close-finalize", "shutdown"]
        );
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(windows)]
#[test]
fn native_end_command_recovers_the_exact_owner_when_cancel_is_available() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (state, project, chat, root) = state_with_admitted_codex_project();
        let project_id = SageProjectId::parse(project).unwrap();
        let chat_id = SageChatId::parse(chat).unwrap();
        let session_id = start_admitted_command_session(&state, &project_id, &chat_id).await;
        let (owner, calls) = install_command_host(
            &state,
            &project_id,
            &chat_id,
            &session_id,
            CommandHostMode::Running,
            true,
        );
        let page = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();

        let result = end_acp_session_inner(
            &state,
            CloseAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: page.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
            },
        )
        .await
        .unwrap();

        assert!(result.projection.current_session_id.is_none());
        assert_eq!(*calls.lock().unwrap(), ["shutdown"]);
        let durable: (String, String, String, Option<String>) = state
            .database
            .connection()
            .query_row(
                "SELECT s.state,s.residency,e.state,e.exit_kind FROM agent_sessions s
                 JOIN session_runtime_epochs e ON e.session_id=s.id
                 WHERE s.id=?1 AND e.runtime_epoch=?2",
                [session_id.as_str(), &owner.runtime_epoch.get().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            durable,
            (
                "failed".into(),
                "cold".into(),
                "failed".into(),
                Some("restartRecovery".into())
            )
        );
        state.acp.shutdown_blocking().unwrap();
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
