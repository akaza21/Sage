use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::acp::{
    contract::{ClientIntentId, RuntimeEpoch, SageChatId, SageProjectId},
    intent::BindingId,
    runtime::{DriverMailbox, LiveRunnerHandle},
    store::{ActivateSessionCommand, StartSessionCommand},
};

#[test]
fn shutdown_failure_reinserts_exact_recovery_and_retry_releases_capacity() {
    tauri::async_runtime::block_on(async {
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let bootstrap = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(bootstrap.active_tab_id.unwrap()).unwrap();
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
                binding_id: BindingId::new("bindingId", "recovery-test").unwrap(),
                binding_digest: "11".repeat(32),
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
        let lease = service
            .sessions
            .activate_session(ActivateSessionCommand {
                project_id,
                chat_id,
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id,
                runtime_epoch: owner.runtime_epoch,
                artifact_digest: "22".repeat(32),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        let turn = lease.reserve_turn_for_test().unwrap();
        service.retain_codex_recovery(PendingCodexRecovery::Abort {
            abort: ActivationAbortIntent {
                owner: owner.clone(),
                binding_digest: "11".repeat(32),
                artifact_digest: "22".repeat(32),
            },
            category: ActivationAbortCategory::AssemblyFailure,
            lease,
            proof: ZeroProcessProof::confirmed(owner),
        });

        assert!(
            service
                .shutdown_codex_recovery(Instant::now() + Duration::from_secs(1))
                .is_err()
        );
        assert!(service.codex_recovery.lock().unwrap().is_some());
        assert!(service.scheduler.active().is_some());

        turn.abort_before_durable_commit().unwrap();
        assert!(
            service
                .shutdown_codex_recovery(Instant::now() + Duration::from_secs(1))
                .unwrap()
        );
        assert!(service.codex_recovery.lock().unwrap().is_none());
        assert!(service.scheduler.active().is_none());
        service.shutdown_blocking().unwrap();
    });
}

#[test]
fn observer_failure_shutdown_recovers_durable_runtime_then_releases_slot() {
    tauri::async_runtime::block_on(async {
        let (service, owner) = ready_runtime_fixture("observer").await;

        let running = crate::acp::process::launch_fixture_for_test(owner.runtime_epoch.get());
        let (_, _, control) = running.into_parts();
        let runner = LiveRunnerHandle::new(Arc::new(DriverMailbox::new().unwrap()), control);
        let host =
            Arc::new(crate::acp::runtime::RuntimeHost::start(TestRuntime::default()).unwrap());
        host.activate(owner.clone()).unwrap();
        service.install_runtime_for_test(owner.clone(), host);
        let prepared = service.codex_gate.prepare_live().unwrap();
        let active = service.codex_gate.begin(&prepared).unwrap();
        super::super::observe_runner_failure_for_test(
            &service.runtime,
            &owner,
            &runner,
            &service.codex_recovery,
            Arc::new(std::sync::Mutex::new(Some(active))),
        );

        assert!(matches!(
            service.runtime.for_owner(&owner),
            Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
        ));
        assert!(service.scheduler.active().is_some());
        assert!(service.codex_recovery.lock().unwrap().is_some());
        service
            .codex_runtime_release_fail_once
            .store(true, std::sync::atomic::Ordering::Release);
        assert!(service.shutdown_blocking().is_err());
        assert!(service.codex_recovery.lock().unwrap().is_some());
        assert!(matches!(
            service.runtime.for_owner(&owner),
            Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
        ));
        assert!(
            service
                .shutdown_codex_recovery(Instant::now() + Duration::from_secs(3))
                .unwrap()
        );
        assert!(service.scheduler.active().is_none());
        assert!(matches!(
            service.runtime.for_owner(&owner),
            Err(super::super::super::runtime_slot::RuntimeSlotError::Foreign)
        ));
        let page = service
            .load_chat_page(owner.project_id, owner.chat_id, None)
            .unwrap();
        let session = page
            .sessions
            .as_slice()
            .iter()
            .find(|session| session.id == owner.session_id)
            .unwrap();
        assert_eq!(session.state, crate::acp::lifecycle::SessionState::Failed);
        assert_eq!(session.residency, crate::acp::lifecycle::Residency::Cold);
        service.shutdown_blocking().unwrap();
    });
}

async fn ready_runtime_fixture(label: &str) -> (AcpService, RuntimeOwner) {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let bootstrap = database.bootstrap_snapshot().unwrap();
    let project_id = SageProjectId::parse(project.id).unwrap();
    let chat_id = SageChatId::parse(bootstrap.active_tab_id.unwrap()).unwrap();
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
            binding_id: BindingId::new("bindingId", format!("{label}-recovery")).unwrap(),
            binding_digest: "31".repeat(32),
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
            artifact_digest: "32".repeat(32),
            now_millis: crate::persistence::now_millis(),
        })
        .await
        .unwrap();
    lease
        .complete_activation(crate::acp::store::CompleteActivationCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            binding_digest: "31".repeat(32),
            artifact_digest: "32".repeat(32),
            external_session_id: crate::acp::intent::ExternalId::new(
                "session",
                format!("{label}-external"),
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

#[test]
fn drop_attempts_persistent_recovery_at_most_twice_before_forced_cleanup() {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let service = AcpService::new(database).unwrap();
    let before = super::super::super::shutdown::DROP_RECOVERY_ATTEMPTS
        .load(std::sync::atomic::Ordering::Acquire);
    service
        .codex_runtime_release_fail_always
        .store(true, std::sync::atomic::Ordering::Release);
    service.retain_codex_recovery(PendingCodexRecovery::RuntimeProjection {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-drop").unwrap(),
            chat_id: SageChatId::parse("chat-drop").unwrap(),
            session_id: crate::acp::contract::SageAgentSessionId::parse("session-drop").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        },
        proof: ZeroProcessProof::confirmed_for_test(RuntimeOwner {
            project_id: SageProjectId::parse("project-drop").unwrap(),
            chat_id: SageChatId::parse("chat-drop").unwrap(),
            session_id: crate::acp::contract::SageAgentSessionId::parse("session-drop").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        }),
    });
    drop(service);
    let attempts = super::super::super::shutdown::DROP_RECOVERY_ATTEMPTS
        .load(std::sync::atomic::Ordering::Acquire)
        .saturating_sub(before);
    assert!(attempts <= 2);
}

#[test]
fn effect_unknown_send_retains_the_exact_host_until_bounded_shutdown_without_replay() {
    let (service, owner, trace) = service_with_operation_recovery(OperationFailure::Send);

    assert!(matches!(
        service.send_codex_turn(prepare_turn(&owner)),
        Err(CodexSessionError::Recovery)
    ));
    assert_eq!(trace.lock().unwrap().send, 1);
    assert!(matches!(
        service.runtime.for_owner(&owner),
        Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
    ));
    assert!(service.codex_recovery.lock().unwrap().is_some());
    assert!(matches!(
        service.send_codex_turn(prepare_turn(&owner)),
        Err(CodexSessionError::Recovery)
    ));
    assert_eq!(trace.lock().unwrap().send, 1, "prompt was replayed");
    let command = close_session(&owner);
    assert!(matches!(
        service.close_codex_session(command),
        Err(CodexSessionError::Recovery)
    ));
    assert!(service.codex_recovery.lock().unwrap().is_none());
    assert!(service.runtime.active().is_none());
    let trace = trace.lock().unwrap();
    assert_eq!(trace.send, 1, "prompt was replayed");
    assert_eq!(trace.close, 0, "normal close was dispatched");
    drop(trace);
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn public_close_retries_managed_shutdown_without_replaying_the_uncertain_effect() {
    let (service, owner) = tauri::async_runtime::block_on(ready_runtime_fixture("managed-retry"));
    let trace = Arc::new(std::sync::Mutex::new(OperationTrace::default()));
    let attempts = Arc::new(AtomicUsize::new(0));
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::scripted(OperationFailure::Send, Arc::clone(&trace)),
            Arc::new(RetryingShutdownRecovery {
                attempts: Arc::clone(&attempts),
                epoch: owner.runtime_epoch.get(),
            }),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);
    assert!(matches!(
        service.send_codex_turn(prepare_turn(&owner)),
        Err(CodexSessionError::Recovery)
    ));
    service
        .codex_runtime_release_fail_once
        .store(true, Ordering::Release);

    let command = close_session(&owner);
    assert!(matches!(
        service.close_codex_session(command.clone()),
        Err(CodexSessionError::Recovery)
    ));
    assert_eq!(attempts.load(Ordering::Acquire), 1);
    assert!(service.codex_recovery.lock().unwrap().is_some());
    assert!(matches!(
        service.runtime.for_owner(&owner),
        Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
    ));

    assert!(matches!(
        service.close_codex_session(command.clone()),
        Err(CodexSessionError::Recovery)
    ));
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(service.codex_recovery.lock().unwrap().is_some());
    assert!(matches!(
        service.runtime.for_owner(&owner),
        Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
    ));

    assert!(matches!(
        service.close_codex_session(command),
        Err(CodexSessionError::Recovery)
    ));
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(service.codex_recovery.lock().unwrap().is_none());
    assert!(service.runtime.active().is_none());
    let trace = trace.lock().unwrap();
    assert_eq!(trace.send, 1, "send was replayed");
    assert_eq!(trace.close, 0, "normal close was dispatched");
    drop(trace);
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

struct RetryingShutdownRecovery {
    attempts: Arc<AtomicUsize>,
    epoch: u64,
}

impl crate::acp::runtime::RuntimeShutdownRecovery for RetryingShutdownRecovery {
    fn shutdown_recovery_before(
        &self,
        _: Instant,
    ) -> Result<crate::acp::process::ZeroProcessToken, crate::acp::runtime::HostError> {
        if self.attempts.fetch_add(1, Ordering::AcqRel) == 0 {
            Err(crate::acp::runtime::HostError::RecoveryRequired)
        } else {
            Ok(crate::acp::process::ZeroProcessToken::for_test(self.epoch))
        }
    }
}

#[test]
fn projection_journal_conversion_failure_retains_the_exact_handoff() {
    tauri::async_runtime::block_on(async {
        let (service, abort, journal, host) = projection_start_fixture(false).await;

        assert!(matches!(
            service.recover_projection_authorities(
                &abort,
                journal,
                crate::acp::process::ZeroProcessToken::for_test(1),
                host,
                None,
            ),
            Err(CodexSessionError::Recovery)
        ));
        assert!(service.codex_recovery.lock().unwrap().is_some());
        assert!(matches!(
            service.runtime.for_owner(&abort.owner),
            Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
        ));
    });
}

#[test]
fn projection_zero_proof_conversion_failure_retains_lease_token_and_host() {
    tauri::async_runtime::block_on(async {
        let (service, abort, journal, host) = projection_start_fixture(true).await;

        assert!(matches!(
            service.recover_projection_authorities(
                &abort,
                journal,
                crate::acp::process::ZeroProcessToken::for_test(2),
                host,
                None,
            ),
            Err(CodexSessionError::Recovery)
        ));
        assert!(service.codex_recovery.lock().unwrap().is_some());
        assert!(matches!(
            service.runtime.for_owner(&abort.owner),
            Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
        ));
    });
}

async fn projection_start_fixture(
    recovery_journal: bool,
) -> (
    AcpService,
    ActivationAbortIntent,
    CoordinatorActivationJournal,
    Arc<RuntimeHost>,
) {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let bootstrap = database.bootstrap_snapshot().unwrap();
    let project_id = SageProjectId::parse(project.id).unwrap();
    let chat_id = SageChatId::parse(bootstrap.active_tab_id.unwrap()).unwrap();
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
            binding_id: BindingId::new("bindingId", "projection-recovery").unwrap(),
            binding_digest: "41".repeat(32),
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
    let lease = service
        .sessions
        .activate_session(ActivateSessionCommand {
            project_id,
            chat_id,
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: started.session_id,
            runtime_epoch: owner.runtime_epoch,
            artifact_digest: "42".repeat(32),
            now_millis: crate::persistence::now_millis(),
        })
        .await
        .unwrap();
    let mut journal = CoordinatorActivationJournal::new(lease);
    if recovery_journal {
        journal.set_recovery_phase_for_test();
    } else {
        journal.set_shutdown_phase_for_test(true);
    }
    let host = Arc::new(RuntimeHost::start(TestRuntime::default()).unwrap());
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));
    let abort = ActivationAbortIntent {
        owner: owner.clone(),
        binding_digest: "41".repeat(32),
        artifact_digest: "42".repeat(32),
    };
    (service, abort, journal, host)
}

#[test]
fn effect_unknown_cancel_retains_its_exact_host_without_redispatch() {
    let (service, owner, trace) = service_with_operation_recovery(OperationFailure::Cancel);
    assert!(service.send_codex_turn(prepare_turn(&owner)).is_ok());
    let intent = crate::acp::runtime::HostCancelIntent {
        owner: owner.clone(),
        turn_id: crate::acp::contract::SageTurnId::parse("turn-operation-recovery").unwrap(),
        client_intent_id: ClientIntentId::new(),
        now_millis: crate::persistence::now_millis(),
    };

    assert!(matches!(
        service.cancel_codex_turn(intent),
        Err(CodexSessionError::Recovery)
    ));
    assert!(matches!(
        service.close_codex_session(close_session(&owner)),
        Err(CodexSessionError::Recovery)
    ));
    assert!(service.codex_recovery.lock().unwrap().is_none());
    assert!(service.runtime.active().is_none());
    let trace = trace.lock().unwrap();
    assert_eq!(trace.cancel, 1, "cancel was redispatched");
    assert_eq!(trace.close, 0, "normal close was dispatched");
    drop(trace);
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn close_effect_retry_failure_reinserts_the_exact_close_authority() {
    let (service, owner, trace) = service_with_operation_recovery(OperationFailure::CloseEffect);
    let command = close_session(&owner);
    assert!(matches!(
        service.close_codex_session(command.clone()),
        Err(CodexSessionError::Recovery)
    ));
    assert!(matches!(
        service.close_codex_session(command),
        Err(CodexSessionError::Intent)
    ));
    assert!(service.codex_recovery.lock().unwrap().is_some());
    assert!(matches!(
        service.runtime.for_owner(&owner),
        Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
    ));
    assert_eq!(trace.lock().unwrap().close, 2);

    assert!(
        service
            .shutdown_codex_recovery(Instant::now() + Duration::from_secs(1))
            .is_ok()
    );
    service.shutdown_blocking().unwrap();
}

#[test]
fn shutdown_failure_after_close_retries_shutdown_without_closing_twice() {
    let (service, owner, trace) = service_with_operation_recovery(OperationFailure::Shutdown);
    let command = close_session(&owner);
    assert!(matches!(
        service.close_codex_session(command.clone()),
        Err(CodexSessionError::Recovery)
    ));
    assert!(matches!(
        service.close_codex_session(command),
        Ok(crate::acp::runtime::HostOutcome::Closed)
    ));
    let trace = trace.lock().unwrap();
    assert_eq!(trace.close, 1, "close was repeated after it completed");
    assert_eq!(trace.shutdown, 2);
    drop(trace);
    assert!(service.codex_recovery.lock().unwrap().is_none());
    assert!(service.runtime.active().is_none());
    service.shutdown_blocking().unwrap();
}

#[derive(Clone, Copy)]
enum OperationFailure {
    Send,
    Cancel,
    CloseEffect,
    Shutdown,
}

#[derive(Default)]
struct OperationTrace {
    send: usize,
    cancel: usize,
    close: usize,
    shutdown: usize,
}

fn service_with_operation_recovery(
    failure: OperationFailure,
) -> (
    AcpService,
    RuntimeOwner,
    Arc<std::sync::Mutex<OperationTrace>>,
) {
    let (service, owner) = tauri::async_runtime::block_on(ready_runtime_fixture("operation"));
    let trace = Arc::new(std::sync::Mutex::new(OperationTrace::default()));
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::scripted(failure, Arc::clone(&trace)),
            Arc::new(SuccessfulShutdownRecovery(owner.runtime_epoch.get())),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);
    (service, owner, trace)
}

struct SuccessfulShutdownRecovery(u64);

impl crate::acp::runtime::RuntimeShutdownRecovery for SuccessfulShutdownRecovery {
    fn shutdown_recovery_before(
        &self,
        _: Instant,
    ) -> Result<crate::acp::process::ZeroProcessToken, crate::acp::runtime::HostError> {
        Ok(crate::acp::process::ZeroProcessToken::for_test(self.0))
    }
}

fn close_session(owner: &RuntimeOwner) -> crate::acp::store::CloseSessionCommand {
    crate::acp::store::CloseSessionCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        expected_projection_revision: crate::acp::contract::ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::new(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        now_millis: crate::persistence::now_millis(),
    }
}

fn assert_recovered_runtime_is_cold(service: &AcpService, owner: &RuntimeOwner) {
    let page = service
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap();
    let session = page
        .sessions
        .as_slice()
        .iter()
        .find(|session| session.id == owner.session_id)
        .unwrap();
    assert_eq!(session.state, crate::acp::lifecycle::SessionState::Failed);
    assert_eq!(session.residency, crate::acp::lifecycle::Residency::Cold);
}

fn prepare_turn(owner: &RuntimeOwner) -> crate::acp::store::PrepareTurnCommand {
    crate::acp::store::PrepareTurnCommand {
        project_id: owner.project_id.clone(),
        project_generation: crate::acp::contract::ProjectGeneration::new(1).unwrap(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        expected_projection_revision: crate::acp::contract::ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::new(),
        prompt: crate::acp::intent::Prompt::new("prompt", "no replay").unwrap(),
        now_millis: crate::persistence::now_millis(),
    }
}

#[derive(Default)]
struct TestRuntime {
    script: Option<(OperationFailure, Arc<std::sync::Mutex<OperationTrace>>)>,
}

impl TestRuntime {
    fn scripted(failure: OperationFailure, trace: Arc<std::sync::Mutex<OperationTrace>>) -> Self {
        Self {
            script: Some((failure, trace)),
        }
    }
}

fn test_capabilities() -> crate::acp::intent::CapabilitySnapshotV1 {
    crate::acp::intent::CapabilitySnapshotV1 {
        schema_version: crate::acp::contract::SchemaVersionV1,
        protocol_version: crate::acp::contract::RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
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

impl crate::acp::runtime::RuntimeEngine for TestRuntime {
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn prepare_prompt(
        &mut self,
        _: &crate::acp::runtime::HostPromptIntent,
    ) -> Result<crate::acp::contract::SageTurnId, crate::acp::runtime::HostError> {
        let Some((failure, trace)) = &self.script else {
            return Err(crate::acp::runtime::HostError::Unavailable);
        };
        trace.lock().unwrap().send += 1;
        if matches!(failure, OperationFailure::Send) {
            return Err(crate::acp::runtime::HostError::RecoveryRequired);
        }
        crate::acp::contract::SageTurnId::parse("turn-operation-recovery")
            .map_err(|_| crate::acp::runtime::HostError::Intent)
    }

    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn: &crate::acp::contract::SageTurnId,
    ) -> Result<crate::acp::runtime::PromptWrite, crate::acp::runtime::HostError> {
        self.script
            .as_ref()
            .map(|_| crate::acp::runtime::PromptWrite::Complete(turn.clone()))
            .ok_or(crate::acp::runtime::HostError::Unavailable)
    }

    fn retry_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<crate::acp::runtime::PromptWrite, crate::acp::runtime::HostError> {
        Err(crate::acp::runtime::HostError::Unavailable)
    }

    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn begin_cancel(
        &mut self,
        _: &crate::acp::runtime::HostCancelIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let Some((failure, trace)) = &self.script else {
            return Ok(());
        };
        trace.lock().unwrap().cancel += 1;
        if matches!(failure, OperationFailure::Cancel) {
            Err(crate::acp::runtime::HostError::RecoveryRequired)
        } else {
            Ok(())
        }
    }

    fn dispatch_cancel(
        &mut self,
        _: &crate::acp::runtime::HostCancelIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn accept_terminal(
        &mut self,
        _: &crate::acp::store::AdapterTurnCursor,
    ) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn begin_close(
        &mut self,
        _: &crate::acp::runtime::HostCloseIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let Some((failure, trace)) = &self.script else {
            return Ok(());
        };
        let mut trace = trace.lock().unwrap();
        trace.close += 1;
        if matches!(failure, OperationFailure::CloseEffect) {
            if trace.close == 1 {
                Err(crate::acp::runtime::HostError::RecoveryRequired)
            } else {
                Err(crate::acp::runtime::HostError::Intent)
            }
        } else {
            Ok(())
        }
    }

    fn converge_zero(
        &mut self,
        _: &crate::acp::runtime::HostCloseIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn finalize_close(
        &mut self,
        _: &crate::acp::runtime::HostCloseIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn shutdown(&mut self, _: Instant) -> Result<(), crate::acp::runtime::HostError> {
        let Some((failure, trace)) = &self.script else {
            return Ok(());
        };
        let mut trace = trace.lock().unwrap();
        trace.shutdown += 1;
        if matches!(failure, OperationFailure::Shutdown) && trace.shutdown == 1 {
            Err(crate::acp::runtime::HostError::RecoveryRequired)
        } else {
            Ok(())
        }
    }
}
