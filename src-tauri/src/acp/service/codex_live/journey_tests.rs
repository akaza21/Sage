use std::{path::Path, sync::Arc, time::Instant};

use super::*;
use crate::acp::{
    bindings::codex::{CodexBindingRegistration, CodexProjectRootAuthority},
    contract::{
        ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SafeText,
        SageAgentSessionId, SageChatId, SageProjectId, SageRootId,
    },
    intent::BindingId,
    process::TreeControl,
    runtime::{
        DriverMailbox, HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent,
        LiveRunnerHandle, PromptWrite, RuntimeEngine, RuntimeHost,
    },
    scheduler::RuntimeOwner,
    store::{
        ActivateSessionCommand, CompleteActivationCommand, PrepareTurnCommand, StartSessionCommand,
    },
};
use crate::project_registry::ProjectRootAuthority;

#[test]
#[cfg(windows)]
fn unavailable_binding_cannot_start_a_live_codex_session() {
    tauri::async_runtime::block_on(async {
        let database = crate::persistence::Database::open(Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service = AcpService::new_with_codex_for_test(
            database,
            CodexBindingRegistration::unavailable_for_test(),
        )
        .unwrap();
        let project_root = codex_root(&project_id, &project.roots[0].id, &root);

        assert!(matches!(
            service
                .start_codex_session(
                    &project_root,
                    start_command(&project_id, &chat_id, ProjectionRevision::ZERO)
                )
                .await,
            Err(CodexSessionError::Binding(_))
        ));
        assert!(matches!(
            service.send_codex_turn(turn_command(&RuntimeOwner {
                project_id,
                chat_id,
                session_id: SageAgentSessionId::parse("session-unavailable").unwrap(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            })),
            Err(CodexSessionError::Intent)
        ));
        assert_eq!(service.runtime.active(), None);
        assert_eq!(
            crate::acp::process::TreeCapacityPool::global()
                .observation_usage()
                .0,
            0
        );
        service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn two_sequential_turns_share_one_session_and_converge_to_zero() {
    tauri::async_runtime::block_on(async {
        let (service, owner) = ready_runtime_fixture("two-turns").await;
        let control = install_fixture_host(&service, owner.clone());

        assert!(service.send_codex_turn(turn_command(&owner)).is_ok());
        assert!(service.send_codex_turn(turn_command(&owner)).is_ok());
        assert!(service.close_codex_session(close_command(&owner)).is_ok());

        assert_eq!(service.runtime.active(), None);
        assert_eq!(control.active_processes().unwrap(), 0);
        assert_same_durable_session(&service, &owner);
        service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn two_chats_are_sequential_on_the_one_runtime_slot() {
    tauri::async_runtime::block_on(async {
        let (service, owner_a) = ready_runtime_fixture("chat-a").await;
        let control_a = install_fixture_host(&service, owner_a.clone());
        let owner_b = RuntimeOwner {
            project_id: owner_a.project_id.clone(),
            chat_id: SageChatId::parse("chat-journey-b").unwrap(),
            session_id: SageAgentSessionId::parse("session-journey-b").unwrap(),
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        };

        assert_eq!(
            service.runtime.reserve_start(owner_b.clone()),
            Err(super::super::runtime_slot::RuntimeSlotError::Occupied)
        );
        assert!(service.close_codex_session(close_command(&owner_a)).is_ok());
        assert_eq!(control_a.active_processes().unwrap(), 0);
        let control_b = install_fixture_host(&service, owner_b.clone());
        assert!(service.send_codex_turn(turn_command(&owner_b)).is_ok());
        assert!(service.close_codex_session(close_command(&owner_b)).is_ok());

        assert_eq!(control_b.active_processes().unwrap(), 0);
        assert_eq!(service.runtime.active(), None);
        service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn restart_resume_then_another_turn_reads_sqlite() {
    tauri::async_runtime::block_on(async {
        let database = crate::persistence::Database::open(Path::new(":memory:")).unwrap();
        let (service, owner) =
            ready_runtime_fixture_from_database(database.clone(), "restart").await;
        let control = install_fixture_host(&service, owner.clone());
        assert!(service.send_codex_turn(turn_command(&owner)).is_ok());
        assert!(service.close_codex_session(close_command(&owner)).is_ok());
        assert_eq!(control.active_processes().unwrap(), 0);
        drop(service);

        let resumed = AcpService::new(database).unwrap();
        assert_same_durable_session(&resumed, &owner);
        let control = install_fixture_host(&resumed, owner.clone());
        assert!(resumed.send_codex_turn(turn_command(&owner)).is_ok());
        assert!(resumed.close_codex_session(close_command(&owner)).is_ok());
        assert_eq!(control.active_processes().unwrap(), 0);
        resumed.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn cancel_then_reuse_session_converges_to_zero_orphans() {
    tauri::async_runtime::block_on(async {
        let (service, owner) = ready_runtime_fixture("cancel-reuse").await;
        let control = install_fixture_host(&service, owner.clone());

        assert!(service.send_codex_turn(turn_command(&owner)).is_ok());
        assert!(
            service
                .cancel_codex_turn(HostCancelIntent {
                    owner: owner.clone(),
                    turn_id: SageAgentSessionId::parse("turn-cancel")
                        .unwrap()
                        .into_turn_id_for_test(),
                    client_intent_id: ClientIntentId::new(),
                    now_millis: crate::persistence::now_millis(),
                })
                .is_ok()
        );
        assert!(service.send_codex_turn(turn_command(&owner)).is_ok());
        assert!(service.close_codex_session(close_command(&owner)).is_ok());

        assert_eq!(control.active_processes().unwrap(), 0);
        assert_eq!(service.runtime.active(), None);
        service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn unsigned_or_observation_only_cannot_launch() {
    tauri::async_runtime::block_on(async {
        for label in ["unsigned", "observation-only"] {
            let database = crate::persistence::Database::open(Path::new(":memory:")).unwrap();
            let root = std::env::current_dir().unwrap().canonicalize().unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let project_id = SageProjectId::parse(project.id).unwrap();
            let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
            let service = AcpService::new_with_codex_for_test(
                database,
                CodexBindingRegistration::unavailable_for_test(),
            )
            .unwrap();
            let project_root = codex_root(&project_id, &project.roots[0].id, &root);

            assert!(
                matches!(
                    service
                        .start_codex_session(
                            &project_root,
                            start_command(&project_id, &chat_id, ProjectionRevision::ZERO)
                        )
                        .await,
                    Err(CodexSessionError::Binding(_))
                ),
                "{label} registration must fail before launch"
            );
            assert_eq!(service.runtime.active(), None);
            assert_eq!(
                crate::acp::process::TreeCapacityPool::global()
                    .observation_usage()
                    .0,
                0
            );
            service.shutdown_blocking().unwrap();
        }
    });
}

async fn ready_runtime_fixture(label: &str) -> (AcpService, RuntimeOwner) {
    ready_runtime_fixture_from_database(
        crate::persistence::Database::open(Path::new(":memory:")).unwrap(),
        label,
    )
    .await
}

async fn ready_runtime_fixture_from_database(
    database: crate::persistence::Database,
    label: &str,
) -> (AcpService, RuntimeOwner) {
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let project_id = SageProjectId::parse(project.id).unwrap();
    let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
    let service = AcpService::new(database).unwrap();
    let revision = service
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    let started = service
        .sessions
        .start_session(StartSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: revision,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new("bindingId", format!("journey-{label}")).unwrap(),
            binding_digest: "51".repeat(32),
            harness_version: None,
            model_id: None,
            now_millis: crate::persistence::now_millis(),
        })
        .await
        .unwrap();
    let owner = RuntimeOwner {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: started.session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    let mut lease = service
        .sessions
        .activate_session(ActivateSessionCommand {
            project_id,
            chat_id,
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: started.session_id,
            runtime_epoch: owner.runtime_epoch,
            artifact_digest: "52".repeat(32),
            now_millis: crate::persistence::now_millis(),
        })
        .await
        .unwrap();
    lease
        .complete_activation(CompleteActivationCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            binding_digest: "51".repeat(32),
            artifact_digest: "52".repeat(32),
            external_session_id: crate::acp::intent::ExternalId::new(
                "session",
                format!("journey-{label}"),
            )
            .unwrap(),
            capabilities_json: serde_json::to_string(&test_capabilities()).unwrap(),
            now_millis: crate::persistence::now_millis(),
        })
        .await
        .unwrap();
    drop(lease);
    (service, owner)
}

fn install_fixture_host(service: &AcpService, owner: RuntimeOwner) -> TreeControl {
    let running = crate::acp::process::launch_fixture_for_test(owner.runtime_epoch.get());
    let (_, _, control) = running.into_parts();
    let runner = Arc::new(LiveRunnerHandle::new(
        Arc::new(DriverMailbox::new().unwrap()),
        control.clone(),
    ));
    let host = Arc::new(
        RuntimeHost::start_with_shutdown_recovery(HappyRuntime::default(), runner).unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner, host);
    control
}

fn start_command(
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    expected_projection_revision: ProjectionRevision,
) -> StartSessionCommand {
    StartSessionCommand {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        expected_projection_revision,
        client_intent_id: ClientIntentId::new(),
        binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
        binding_digest: "forged".into(),
        harness_version: Some(SafeText::new("version", "forged").unwrap()),
        model_id: None,
        now_millis: crate::persistence::now_millis(),
    }
}

fn turn_command(owner: &RuntimeOwner) -> PrepareTurnCommand {
    PrepareTurnCommand {
        project_id: owner.project_id.clone(),
        project_generation: ProjectGeneration::ZERO,
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        expected_projection_revision: ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::new(),
        prompt: crate::acp::intent::Prompt::new("prompt", "journey turn").unwrap(),
        now_millis: crate::persistence::now_millis(),
    }
}

fn close_command(owner: &RuntimeOwner) -> crate::acp::store::CloseSessionCommand {
    crate::acp::store::CloseSessionCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        expected_projection_revision: ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::new(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        now_millis: crate::persistence::now_millis(),
    }
}

fn codex_root(project_id: &SageProjectId, root_id: &str, root: &Path) -> CodexProjectRootAuthority {
    CodexProjectRootAuthority::for_test(
        ProjectRootAuthority::for_test(
            project_id.clone(),
            ProjectGeneration::ZERO,
            SageRootId::parse(root_id).unwrap(),
            root,
        )
        .unwrap(),
    )
}

fn assert_same_durable_session(service: &AcpService, owner: &RuntimeOwner) {
    assert!(
        service
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap()
            .sessions
            .as_slice()
            .iter()
            .any(|session| session.id == owner.session_id)
    );
}

#[derive(Default)]
struct HappyRuntime;

impl RuntimeEngine for HappyRuntime {
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        Ok(())
    }
    fn prepare_prompt(
        &mut self,
        _: &HostPromptIntent,
    ) -> Result<crate::acp::contract::SageTurnId, HostError> {
        crate::acp::contract::SageTurnId::parse("turn-journey").map_err(|_| HostError::Intent)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn: &crate::acp::contract::SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Ok(PromptWrite::Complete(turn.clone()))
    }
    fn retry_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn: &crate::acp::contract::SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Ok(PromptWrite::Complete(turn.clone()))
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn accept_terminal(
        &mut self,
        _: &crate::acp::store::AdapterTurnCursor,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, _: Instant) -> Result<(), HostError> {
        Ok(())
    }
}

fn test_capabilities() -> crate::acp::intent::CapabilitySnapshotV1 {
    crate::acp::intent::CapabilitySnapshotV1 {
        schema_version: crate::acp::contract::SchemaVersionV1,
        protocol_version: crate::acp::contract::RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: true,
        load_session: true,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        config_count: 0,
    }
}
