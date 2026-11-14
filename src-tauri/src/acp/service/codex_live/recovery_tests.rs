use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, SyncSender},
    },
    time::Duration,
};

use super::live_recovery::{LiveHostRecovery, LiveHostRecoveryPhase};
use super::*;
use crate::acp::{
    runtime::{DriverMailbox, LiveRunnerHandle},
    store::{ActivateSessionCommand, StartSessionCommand},
};
use sage_acp_protocol::{
    contract::{ClientIntentId, RuntimeEpoch, SageChatId, SageProjectId},
    intent::BindingId,
};

#[test]
fn shutdown_failure_reinserts_exact_recovery_and_retry_releases_capacity() {
    tauri::async_runtime::block_on(async {
        let (service, owner, lease) = activating_session_fixture("recovery-test").await;
        let turn = lease.reserve_turn_for_test().unwrap();
        service
            .lane_at(0)
            .retain_codex_recovery(PendingCodexRecovery::Abort {
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
        assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
        assert!(service.lane_at(0).scheduler.active().is_some());

        turn.abort_before_durable_commit().unwrap();
        assert!(
            service
                .shutdown_codex_recovery(Instant::now() + Duration::from_secs(1))
                .unwrap()
        );
        assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
        assert!(service.lane_at(0).scheduler.active().is_none());
        service.shutdown_blocking().unwrap();
    });
}

#[test]
fn retained_activation_recovery_releases_capacity_before_another_live_admission() {
    tauri::async_runtime::block_on(async {
        let (service, owner, lease) = activating_session_fixture("activation-reclaim").await;
        service
            .lane_at(0)
            .runtime
            .reserve_start(owner.clone())
            .unwrap();
        service.lane_at(0).runtime.mark_recovery(&owner).unwrap();
        service
            .lane_at(0)
            .retain_codex_recovery(PendingCodexRecovery::Abort {
                abort: ActivationAbortIntent {
                    owner: owner.clone(),
                    binding_digest: "11".repeat(32),
                    artifact_digest: "22".repeat(32),
                },
                category: ActivationAbortCategory::AssemblyFailure,
                lease,
                proof: ZeroProcessProof::confirmed(owner.clone()),
            });

        assert!(
            service
                .reclaim_zero_process_capacity(Instant::now() + Duration::from_secs(1))
                .unwrap()
        );
        assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
        let next = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(owner.runtime_epoch.get() + 1).unwrap(),
            ..owner
        };
        service
            .lane_at(0)
            .runtime
            .reserve_start(next.clone())
            .unwrap();
        service.lane_at(0).runtime.abandon_start(&next).unwrap();
        service.shutdown_blocking().unwrap();
    });
}

async fn activating_session_fixture(
    label: &str,
) -> (
    AcpService,
    RuntimeOwner,
    crate::acp::runtime::SessionTreeLease,
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
        .lane_at(0)
        .sessions
        .start_session(StartSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: revision,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new("bindingId", label).unwrap(),
            binding_digest: "11".repeat(32),
            harness_version: None,
            model_id: None,
            now_millis: sage_core::time::now_millis(),
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
        .lane_at(0)
        .sessions
        .activate_session(ActivateSessionCommand {
            project_id,
            chat_id,
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: started.session_id,
            runtime_epoch: owner.runtime_epoch,
            artifact_digest: "22".repeat(32),
            now_millis: sage_core::time::now_millis(),
        })
        .await
        .unwrap();
    (service, owner, lease)
}

#[test]
fn cancel_after_runner_failure_recovers_durable_runtime_then_releases_slot() {
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
            &service.lane_at(0).runtime,
            &owner,
            &runner,
            &service.lane_at(0).codex_recovery,
            Arc::new(std::sync::Mutex::new(Some(active))),
        );

        assert!(matches!(
            service.lane_at(0).runtime.for_owner(&owner),
            Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
        ));
        assert!(service.lane_at(0).scheduler.active().is_some());
        assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
        assert!(matches!(
            service.cancel_codex_turn(crate::acp::runtime::HostCancelIntent {
                owner: owner.clone(),
                turn_id: sage_acp_protocol::contract::SageTurnId::parse("turn-runner-recovery")
                    .unwrap(),
                client_intent_id: ClientIntentId::new(),
                now_millis: sage_core::time::now_millis(),
            }),
            Err(CodexSessionError::Recovery)
        ));
        assert!(service.lane_at(0).scheduler.active().is_none());
        assert!(matches!(
            service.lane_at(0).runtime.for_owner(&owner),
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
        assert_eq!(
            session.state,
            sage_acp_protocol::lifecycle::SessionState::Failed
        );
        assert_eq!(
            session.residency,
            sage_acp_protocol::lifecycle::Residency::Cold
        );
        service.shutdown_blocking().unwrap();
    });
}

#[test]
fn zero_process_runner_failure_is_reclaimed_before_another_live_admission() {
    tauri::async_runtime::block_on(async {
        let (service, owner) = ready_runtime_fixture("zero-process-reclaim").await;
        let running = crate::acp::process::launch_fixture_for_test(owner.runtime_epoch.get());
        let (_, _, control) = running.into_parts();
        control
            .terminate_zero(crate::acp::process::TreeExit::ProtocolFailure)
            .expect("fixture tree reaches zero");
        let runner = LiveRunnerHandle::new(Arc::new(DriverMailbox::new().unwrap()), control);
        let host =
            Arc::new(crate::acp::runtime::RuntimeHost::start(TestRuntime::default()).unwrap());
        host.activate(owner.clone()).unwrap();
        service.install_runtime_for_test(owner.clone(), host);
        let prepared = service.codex_gate.prepare_live().unwrap();
        let active = service.codex_gate.begin(&prepared).unwrap();
        super::super::observe_runner_failure_for_test(
            &service.lane_at(0).runtime,
            &owner,
            &runner,
            &service.lane_at(0).codex_recovery,
            Arc::new(std::sync::Mutex::new(Some(active))),
        );

        assert!(
            service
                .lane_at(0)
                .reclaim_zero_process_runner_capacity(Instant::now() + Duration::from_secs(1))
                .unwrap()
        );
        assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
        assert!(service.lane_at(0).scheduler.active().is_none());
        assert!(matches!(
            service.lane_at(0).runtime.for_owner(&owner),
            Err(super::super::super::runtime_slot::RuntimeSlotError::Foreign)
        ));
        service.shutdown_blocking().unwrap();
    });
}

#[test]
fn live_process_runner_failure_remains_capacity_blocked() {
    tauri::async_runtime::block_on(async {
        let (service, owner) = ready_runtime_fixture("live-process-retain").await;
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
            &service.lane_at(0).runtime,
            &owner,
            &runner,
            &service.lane_at(0).codex_recovery,
            Arc::new(std::sync::Mutex::new(Some(active))),
        );

        assert!(matches!(
            service
                .lane_at(0)
                .reclaim_zero_process_runner_capacity(Instant::now() + Duration::from_millis(50)),
            Err(crate::acp::runtime::HostError::RecoveryRequired)
        ));
        assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
        assert!(service.lane_at(0).scheduler.active().is_some());
        service.shutdown_blocking().unwrap();
    });
}

async fn ready_runtime_fixture(label: &str) -> (AcpService, RuntimeOwner) {
    ready_runtime_fixture_with(label, test_capabilities()).await
}

async fn ready_runtime_fixture_with(
    label: &str,
    capabilities: sage_acp_protocol::intent::CapabilitySnapshotV1,
) -> (AcpService, RuntimeOwner) {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let root = std::env::current_dir().unwrap().canonicalize().unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let bootstrap = database.bootstrap_snapshot().unwrap();
    let project_id = SageProjectId::parse(project.id).unwrap();
    let parent = bootstrap.active_tab_id.unwrap();
    let chat = if label.starts_with("side-") {
        database
            .create_chat_with_parent(
                project_id.as_str(),
                &sage_domain::chat::ProjectLayoutRequest {
                    project_id: project_id.as_str().to_owned(),
                    tabs: bootstrap.tabs,
                    active_tab_id: Some(parent.clone()),
                    selected_root_id: bootstrap.selected_root_id,
                    selected_resource: bootstrap.selected_resource,
                },
                Some(&parent),
            )
            .unwrap()
    } else {
        parent
    };
    let chat_id = SageChatId::parse(chat).unwrap();
    let service = AcpService::new(database).unwrap();
    let owner = seat_ready_session_with(&service, project_id, chat_id, label, capabilities).await;
    (service, owner)
}

async fn seat_ready_session(
    service: &AcpService,
    project_id: SageProjectId,
    chat_id: SageChatId,
    label: &str,
) -> RuntimeOwner {
    seat_ready_session_with(service, project_id, chat_id, label, test_capabilities()).await
}

async fn seat_ready_session_with(
    service: &AcpService,
    project_id: SageProjectId,
    chat_id: SageChatId,
    label: &str,
    capabilities: sage_acp_protocol::intent::CapabilitySnapshotV1,
) -> RuntimeOwner {
    let revision = service
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    let started = service
        .lane_at(0)
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
            now_millis: sage_core::time::now_millis(),
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
        .lane_at(0)
        .sessions
        .activate_session(ActivateSessionCommand {
            project_id,
            chat_id,
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: started.session_id,
            runtime_epoch: owner.runtime_epoch,
            artifact_digest: "32".repeat(32),
            now_millis: sage_core::time::now_millis(),
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
            external_session_id: sage_acp_protocol::intent::ExternalId::new(
                "session",
                format!("{label}-external"),
            )
            .unwrap(),
            capabilities_json: serde_json::to_string(&capabilities).unwrap(),
            now_millis: sage_core::time::now_millis(),
        })
        .await
        .unwrap();
    drop(lease);
    owner
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
    service
        .lane_at(0)
        .retain_codex_recovery(PendingCodexRecovery::RuntimeProjection {
            owner: RuntimeOwner {
                project_id: SageProjectId::parse("project-drop").unwrap(),
                chat_id: SageChatId::parse("chat-drop").unwrap(),
                session_id: sage_acp_protocol::contract::SageAgentSessionId::parse("session-drop")
                    .unwrap(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            },
            proof: ZeroProcessProof::confirmed_for_test(RuntimeOwner {
                project_id: SageProjectId::parse("project-drop").unwrap(),
                chat_id: SageChatId::parse("chat-drop").unwrap(),
                session_id: sage_acp_protocol::contract::SageAgentSessionId::parse("session-drop")
                    .unwrap(),
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
        service.lane_at(0).runtime.for_owner(&owner),
        Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
    ));
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
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
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).runtime.active().is_none());
    let trace = trace.lock().unwrap();
    assert_eq!(trace.send, 1, "prompt was replayed");
    assert_eq!(trace.close, 0, "normal close was dispatched");
    drop(trace);
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn native_end_proves_zero_process_and_terminalizes_the_exact_runtime() {
    let (service, owner) = tauri::async_runtime::block_on(ready_runtime_fixture("native-end"));
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::default(),
            Arc::new(SuccessfulShutdownRecovery(owner.runtime_epoch.get())),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);

    service.end_session_natively(owner.clone()).unwrap();

    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).scheduler.active().is_none());
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn temporary_disposal_retries_supervisor_shutdown_without_protocol_close() {
    let (service, owner) = tauri::async_runtime::block_on(ready_runtime_fixture("side-disposal"));
    let attempts = Arc::new(AtomicUsize::new(0));
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::default(),
            Arc::new(RetryingShutdownRecovery {
                attempts: Arc::clone(&attempts),
                epoch: owner.runtime_epoch.get(),
            }),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);
    let mut command = prepare_turn(&owner);
    command.expected_projection_revision = service
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    service.database.acp_prepare_turn(command).unwrap();
    assert!(
        service
            .dispose_temporary_runtime(&owner.project_id, &owner.chat_id)
            .is_err()
    );
    assert_eq!(attempts.load(Ordering::Acquire), 1);
    service
        .dispose_temporary_runtime(&owner.project_id, &owner.chat_id)
        .unwrap();
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(service.lane_at(0).scheduler.active().is_none());
    assert_recovered_runtime_is_cold(&service, &owner);
    let page = service
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap();
    assert_eq!(page.turns.as_slice().len(), 1);
    assert!(matches!(
        page.turns.as_slice()[0].state,
        sage_acp_protocol::lifecycle::TurnState::Interrupted
            | sage_acp_protocol::lifecycle::TurnState::Cancelled
    ));
    service
        .database
        .remove_side_chat(owner.project_id.as_str(), owner.chat_id.as_str())
        .unwrap();
    let usage: (i64, i64, i64, i64) = service.database.connection().query_row(
        "SELECT session_count,turn_count,item_count,fact_count FROM acp_store_usage WHERE singleton=1",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
    ).unwrap();
    assert_eq!(usage, (0, 0, 0, 0));
    assert!(
        service
            .database
            .project_list()
            .unwrap()
            .iter()
            .all(|project| project
                .rail_chats
                .iter()
                .all(|chat| chat.id != owner.chat_id.as_str()))
    );
    service.shutdown_blocking().unwrap();
}

#[test]
fn native_end_refuses_to_replace_an_existing_recovery() {
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture("native-end-existing"));
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::default(),
            Arc::new(SuccessfulShutdownRecovery(owner.runtime_epoch.get())),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));
    service.lane_at(0).runtime.mark_recovery(&owner).unwrap();
    service
        .lane_at(0)
        .retain_codex_recovery(PendingCodexRecovery::LiveHost(LiveHostRecovery {
            owner: owner.clone(),
            host,
            phase: LiveHostRecoveryPhase::SendEffect,
        }));

    assert!(matches!(
        service.end_session_natively(owner.clone()),
        Err(CodexSessionError::Recovery)
    ));
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
    assert!(service.lane_at(0).scheduler.active().is_some());
    service.shutdown_blocking().unwrap();
}

#[test]
fn native_end_without_native_proof_refuses_without_mutating_the_runtime() {
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture("native-end-no-proof"));
    let host = Arc::new(crate::acp::runtime::RuntimeHost::start(TestRuntime::default()).unwrap());
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));

    assert!(matches!(
        service.end_session_natively(owner.clone()),
        Err(CodexSessionError::Recovery)
    ));
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).scheduler.active().is_some());
    assert!(service.lane_at(0).runtime.for_owner(&owner).is_ok());
    assert!(service.lane_at(0).runtime.for_owner(&owner).is_ok());
    service.shutdown_blocking().unwrap();
}

#[test]
fn native_end_retries_its_exact_retained_recovery() {
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture("native-end-retry"));
    let attempts = Arc::new(AtomicUsize::new(0));
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::default(),
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
        service.end_session_natively(owner.clone()),
        Err(CodexSessionError::Recovery)
    ));
    assert_eq!(attempts.load(Ordering::Acquire), 1);
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());

    service.end_session_natively(owner.clone()).unwrap();
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn native_end_preserves_its_terminal_outcome_against_inflight_and_late_config_recovery() {
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture("native-end-in-flight"));
    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);
    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::default(),
            Arc::new(BlockingShutdownRecovery {
                entered: entered_sender,
                release: Mutex::new(release_receiver),
                epoch: owner.runtime_epoch.get(),
            }),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));

    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| service.end_session_natively(owner.clone()));
        entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        assert_native_end_in_flight(&service, &owner, &host);
        service.retain_config_effect_recovery(owner.clone(), Arc::clone(&host));
        service.lane_at(0).forget_stale_owner_recovery(&owner);
        assert!(matches!(
            service.reclaim_zero_process_capacity(Instant::now() + Duration::from_millis(50)),
            Err(crate::acp::runtime::HostError::RecoveryRequired)
        ));
        assert_native_end_in_flight(&service, &owner, &host);
        assert!(service.lane_at(0).scheduler.active().is_some());

        release_sender.send(()).unwrap();
        worker.join().unwrap()
    });

    result.unwrap();
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).scheduler.active().is_none());
    service.retain_config_effect_recovery(owner.clone(), host);
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn shutdown_retry_keeps_native_end_inflight_when_a_runner_fails() {
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture("native-end-shutdown"));
    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);
    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::default(),
            Arc::new(BlockingShutdownRecovery {
                entered: entered_sender,
                release: Mutex::new(release_receiver),
                epoch: owner.runtime_epoch.get(),
            }),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));
    service.lane_at(0).runtime.mark_recovery(&owner).unwrap();
    service
        .lane_at(0)
        .retain_codex_recovery(PendingCodexRecovery::LiveHost(LiveHostRecovery {
            owner: owner.clone(),
            host: Arc::clone(&host),
            phase: LiveHostRecoveryPhase::NativeEnd,
        }));
    let running = crate::acp::process::launch_fixture_for_test(owner.runtime_epoch.get());
    let (_, _, control) = running.into_parts();
    control
        .terminate_zero(crate::acp::process::TreeExit::ProtocolFailure)
        .unwrap();
    let runner = LiveRunnerHandle::new(Arc::new(DriverMailbox::new().unwrap()), control);

    let result = std::thread::scope(|scope| {
        let worker = scope
            .spawn(|| service.shutdown_codex_recovery(Instant::now() + Duration::from_secs(1)));
        entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        super::super::observe_runner_failure_for_test(
            &service.lane_at(0).runtime,
            &owner,
            &runner,
            &service.lane_at(0).codex_recovery,
            Arc::new(std::sync::Mutex::new(None)),
        );
        assert_native_end_in_flight(&service, &owner, &host);

        release_sender.send(()).unwrap();
        worker.join().unwrap()
    });

    assert!(result.unwrap());
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).scheduler.active().is_none());
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn close_recovery_keeps_live_authority_inflight_when_a_runner_fails() {
    let (service, owner) = tauri::async_runtime::block_on(ready_runtime_fixture("close-inflight"));
    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);
    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start(TestRuntime::blocking_close(
            entered_sender,
            release_receiver,
        ))
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));
    let command = close_session(&owner);
    assert!(matches!(
        service.close_codex_session(command.clone()),
        Err(CodexSessionError::Recovery)
    ));
    let running = crate::acp::process::launch_fixture_for_test(owner.runtime_epoch.get());
    let (_, _, control) = running.into_parts();
    control
        .terminate_zero(crate::acp::process::TreeExit::ProtocolFailure)
        .unwrap();
    let runner = LiveRunnerHandle::new(Arc::new(DriverMailbox::new().unwrap()), control);

    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| service.close_codex_session(command));
        entered_receiver
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        super::super::observe_runner_failure_for_test(
            &service.lane_at(0).runtime,
            &owner,
            &runner,
            &service.lane_at(0).codex_recovery,
            Arc::new(std::sync::Mutex::new(None)),
        );
        service.lane_at(0).retain_live_failure_recovery(
            owner.clone(),
            Arc::clone(&host),
            LiveHostRecoveryPhase::SendEffect,
        );

        release_sender.send(()).unwrap();
        worker.join().unwrap()
    });

    assert!(matches!(
        result,
        Ok(crate::acp::runtime::HostOutcome::Closed)
    ));
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).runtime.active().is_none());
    service.shutdown_blocking().unwrap();
}

fn assert_native_end_in_flight(
    service: &AcpService,
    owner: &RuntimeOwner,
    host: &Arc<RuntimeHost>,
) {
    let lane = service.lane_at(0);
    let retained = lane.codex_recovery.lock().unwrap();
    let Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
        owner: current_owner,
        host: current_host,
        phase: LiveHostRecoveryPhase::NativeEndInFlight,
    })) = retained.as_ref()
    else {
        panic!("native End recovery was not retained in flight");
    };
    assert_eq!(current_owner, owner);
    assert!(Arc::ptr_eq(current_host, host));
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
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
    assert!(matches!(
        service.lane_at(0).runtime.for_owner(&owner),
        Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
    ));

    assert!(matches!(
        service.close_codex_session(command.clone()),
        Err(CodexSessionError::Recovery)
    ));
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
    assert!(matches!(
        service.lane_at(0).runtime.for_owner(&owner),
        Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
    ));

    assert!(matches!(
        service.close_codex_session(command),
        Err(CodexSessionError::Recovery)
    ));
    assert_eq!(attempts.load(Ordering::Acquire), 2);
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).runtime.active().is_none());
    let trace = trace.lock().unwrap();
    assert_eq!(trace.send, 1, "send was replayed");
    assert_eq!(trace.close, 0, "normal close was dispatched");
    drop(trace);
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn replacement_park_after_host_timeout_uses_zero_token_recovery() {
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture("replacement-timeout"));
    let (entered_sender, entered_receiver) = std::sync::mpsc::sync_channel(1);
    let (release_sender, release_receiver) = std::sync::mpsc::sync_channel(1);
    let host = Arc::new(
        RuntimeHost::start_with_timeout_and_shutdown_recovery(
            TestRuntime::blocking_prepare(entered_sender, release_receiver),
            Duration::from_millis(20),
            Arc::new(SuccessfulShutdownRecovery(owner.runtime_epoch.get())),
        )
        .unwrap(),
    );
    host.without_timeout_override(|| host.activate(owner.clone()))
        .unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));
    assert!(service.lane_at(0).scheduler.active().is_some());

    let timed_out = {
        let host = Arc::clone(&host);
        let intent = crate::acp::runtime::HostPromptIntent {
            command: prepare_turn(&owner),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        };
        std::thread::spawn(move || host.prepare_prompt(intent))
    };
    entered_receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert!(matches!(
        timed_out.join().unwrap(),
        Err(crate::acp::runtime::HostError::Deadline)
    ));
    assert!(host.is_awaiting_exit());
    release_sender.send(()).unwrap();

    service.lane_at(0).retain_live_failure_recovery(
        owner.clone(),
        Arc::clone(&host),
        LiveHostRecoveryPhase::ReplacementPark,
    );
    assert!(service.lane_at(0).scheduler.active().is_some());
    assert!(matches!(
        service.shutdown_codex_recovery(Instant::now() + Duration::from_secs(1)),
        Ok(true)
    ));
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).scheduler.active().is_none());
    assert_recovered_runtime_is_cold(&service, &owner);
    service.shutdown_blocking().unwrap();
}

#[test]
fn unsettled_replacement_park_preserves_its_owner_and_other_admission_capacity() {
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture("replacement-capacity"));
    let host = Arc::new(
        RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::scripted(
                OperationFailure::Shutdown,
                Arc::new(Mutex::new(OperationTrace::default())),
            ),
            Arc::new(RetryingShutdownRecovery {
                attempts: Arc::new(AtomicUsize::new(0)),
                epoch: owner.runtime_epoch.get(),
            }),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));
    service.lane_at(0).retain_live_failure_recovery(
        owner.clone(),
        host,
        LiveHostRecoveryPhase::ReplacementPark,
    );
    let revision = service
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    let newcomer = RuntimeOwner {
        chat_id: SageChatId::parse("chat-newcomer").unwrap(),
        session_id: sage_acp_protocol::contract::SageAgentSessionId::parse("session-newcomer")
            .unwrap(),
        ..owner.clone()
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let (available, claim) = service.claim_activation_lane(&newcomer, deadline).unwrap();
    assert!(available.runtime.is_empty());
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
    drop(claim);
    let (held, claim) = service.claim_activation_lane(&owner, deadline).unwrap();
    assert_eq!(held.runtime.owner(), Some(owner.clone()));
    assert!(matches!(
        held.runtime.for_owner(&owner),
        Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
    ));
    assert_eq!(
        service
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap()
            .projection_revision,
        revision
    );
    drop(claim);
    service.shutdown_blocking().unwrap();
}

#[test]
fn steer_ack_timeout_keeps_runtime_live_for_an_explicit_retry() {
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture("steer-ack-timeout"));
    let pending = Arc::new(Mutex::new(None));
    let attempts = Arc::new(AtomicUsize::new(0));
    let host = Arc::new(
        RuntimeHost::start_with_timeout_and_shutdown_recovery(
            TestRuntime::delayed_first_steer(Arc::clone(&pending), Arc::clone(&attempts)),
            Duration::from_millis(30),
            Arc::new(SuccessfulShutdownRecovery(owner.runtime_epoch.get())),
        )
        .unwrap(),
    );
    host.without_timeout_override(|| {
        host.activate(owner.clone())?;
        host.send(crate::acp::runtime::HostPromptIntent {
            command: prepare_turn(&owner),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .map_err(|failure| failure.error)?;
        Ok::<(), crate::acp::runtime::HostError>(())
    })
    .unwrap();
    service.install_runtime_for_test(owner.clone(), Arc::clone(&host));
    let intent = crate::acp::runtime::HostSteerIntent {
        owner: owner.clone(),
        turn_id: sage_acp_protocol::contract::SageTurnId::parse("turn-operation-recovery").unwrap(),
        prompt: crate::acp::transcript_quote::Prompt::new("prompt", "Please steer").unwrap(),
        client_message_id: sage_acp_protocol::contract::RequiredText::new(
            "clientMessageId",
            "queued-one",
        )
        .unwrap(),
    };

    let error = host.steer(intent.clone()).unwrap_err();
    let _ = service.lane_at(0).retain_live_host_error(
        owner.clone(),
        Arc::clone(&host),
        LiveHostRecoveryPhase::SendEffect,
        error,
    );
    assert!(service.lane_at(0).runtime.for_owner(&owner).is_ok());
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert_eq!(
        host.steer(intent),
        Ok(crate::acp::runtime::HostOutcome::SteerQueued)
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    drop(pending.lock().unwrap().take());
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
            service.lane_at(0).recover_projection_authorities(
                &abort,
                journal,
                crate::acp::process::ZeroProcessToken::for_test(1),
                host,
                None,
            ),
            Err(CodexSessionError::Recovery)
        ));
        assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
        assert!(matches!(
            service.lane_at(0).runtime.for_owner(&abort.owner),
            Err(super::super::super::runtime_slot::RuntimeSlotError::Recovery)
        ));
    });
}

#[test]
fn projection_zero_proof_conversion_failure_retains_lease_token_and_host() {
    tauri::async_runtime::block_on(async {
        let (service, abort, journal, host) = projection_start_fixture(true).await;

        assert!(matches!(
            service.lane_at(0).recover_projection_authorities(
                &abort,
                journal,
                crate::acp::process::ZeroProcessToken::for_test(2),
                host,
                None,
            ),
            Err(CodexSessionError::Recovery)
        ));
        assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
        assert!(matches!(
            service.lane_at(0).runtime.for_owner(&abort.owner),
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
        .lane_at(0)
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
            now_millis: sage_core::time::now_millis(),
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
        .lane_at(0)
        .sessions
        .activate_session(ActivateSessionCommand {
            project_id,
            chat_id,
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: started.session_id,
            runtime_epoch: owner.runtime_epoch,
            artifact_digest: "42".repeat(32),
            now_millis: sage_core::time::now_millis(),
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
        turn_id: sage_acp_protocol::contract::SageTurnId::parse("turn-operation-recovery").unwrap(),
        client_intent_id: ClientIntentId::new(),
        now_millis: sage_core::time::now_millis(),
    };

    assert!(matches!(
        service.cancel_codex_turn(intent),
        Err(CodexSessionError::Recovery)
    ));
    assert!(matches!(
        service.close_codex_session(close_session(&owner)),
        Err(CodexSessionError::Recovery)
    ));
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).runtime.active().is_none());
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
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_some());
    assert!(matches!(
        service.lane_at(0).runtime.for_owner(&owner),
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
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_at(0).runtime.active().is_none());
    service.shutdown_blocking().unwrap();
}

#[derive(Clone, Copy)]
enum OperationFailure {
    Send,
    Cancel,
    CloseEffect,
    Shutdown,
    /// The harness refuses the prompt before writing it (B1).
    SignIn,
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

struct BlockingShutdownRecovery {
    entered: SyncSender<()>,
    release: Mutex<Receiver<()>>,
    epoch: u64,
}

struct BlockingCloseRecovery {
    attempts: usize,
    entered: SyncSender<()>,
    release: Mutex<Receiver<()>>,
}

struct BlockingPrepareRecovery {
    entered: SyncSender<()>,
    release: Mutex<Receiver<()>>,
}

impl crate::acp::runtime::RuntimeShutdownRecovery for BlockingShutdownRecovery {
    fn shutdown_recovery_before(
        &self,
        _: Instant,
    ) -> Result<crate::acp::process::ZeroProcessToken, crate::acp::runtime::HostError> {
        self.entered
            .send(())
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(1))
            .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        Ok(crate::acp::process::ZeroProcessToken::for_test(self.epoch))
    }
}

fn close_session(owner: &RuntimeOwner) -> crate::acp::store::CloseSessionCommand {
    crate::acp::store::CloseSessionCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        expected_projection_revision: sage_acp_protocol::contract::ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::new(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        now_millis: sage_core::time::now_millis(),
    }
}

fn app_with_idle_runtime(
    label: &str,
) -> (crate::state::AppState, RuntimeOwner, std::path::PathBuf) {
    app_with_runtime(label, true)
}

/// Without `parkable` the runtime has no shutdown recovery, so a park of it
/// never settles.
fn app_with_runtime(
    label: &str,
    parkable: bool,
) -> (crate::state::AppState, RuntimeOwner, std::path::PathBuf) {
    app_with_engine(label, parkable, TestRuntime::default())
}

fn app_with_engine(
    label: &str,
    parkable: bool,
    engine: TestRuntime,
) -> (crate::state::AppState, RuntimeOwner, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("sage-archive-runtime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = crate::persistence::Database::open(&root.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let bootstrap = database.bootstrap_snapshot().unwrap();
    let chat = bootstrap.active_tab_id.clone().unwrap();
    database
        .create_chat(&layout_of(&project.id, bootstrap))
        .unwrap();
    let state = crate::state::AppState::new(database);
    let owner = tauri::async_runtime::block_on(seat_ready_session(
        &state.acp,
        SageProjectId::parse(project.id).unwrap(),
        SageChatId::parse(chat).unwrap(),
        label,
    ));
    let host = Arc::new(
        if parkable {
            crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
                engine,
                Arc::new(SuccessfulShutdownRecovery(owner.runtime_epoch.get())),
            )
        } else {
            crate::acp::runtime::RuntimeHost::start(engine)
        }
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    state.acp.install_runtime_for_test(owner.clone(), host);
    (state, owner, root)
}

#[test]
fn delete_refuses_a_park_that_cannot_settle_at_once_without_retrying() {
    let (state, owner, root) = app_with_runtime("unsettled-park", false);
    state
        .database
        .archive_chat(&archive_request(&state, &owner))
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let refused = runtime.block_on(crate::chat_archive_commands::delete_chat_inner(
        &state,
        delete_request(&owner),
        0,
    ));
    assert!(!refused.committed);
    assert_eq!(
        refused.recovery_error.as_deref(),
        Some("This Chat's agent is still shutting down; try deleting it again")
    );
    assert_eq!(
        state.acp.lane_busy_refusals_for_test(),
        0,
        "an unsettled park is not a busy lane"
    );
    assert!(state.acp.lane_for(&owner).is_some(), "the Chat is kept");
    let _ = state.acp.shutdown_blocking();
    drop(state);
    let _ = std::fs::remove_dir_all(root);
}

fn layout_of(
    project: &str,
    snapshot: sage_domain::project::BootstrapSnapshot,
) -> sage_domain::chat::ProjectLayoutRequest {
    sage_domain::chat::ProjectLayoutRequest {
        project_id: project.to_owned(),
        tabs: snapshot.tabs,
        active_tab_id: snapshot.active_tab_id,
        selected_root_id: snapshot.selected_root_id,
        selected_resource: snapshot.selected_resource,
    }
}

fn archive_request(
    state: &crate::state::AppState,
    owner: &RuntimeOwner,
) -> sage_domain::chat::ChatLifecycleRequest {
    sage_domain::chat::ChatLifecycleRequest {
        project_id: owner.project_id.as_str().to_owned(),
        chat_id: owner.chat_id.as_str().to_owned(),
        current_layout: layout_of(
            owner.project_id.as_str(),
            state.database.bootstrap_snapshot().unwrap(),
        ),
    }
}

fn delete_request(owner: &RuntimeOwner) -> sage_domain::chat::ChatDeleteRequest {
    sage_domain::chat::ChatDeleteRequest {
        project_id: owner.project_id.as_str().to_owned(),
        chat_id: owner.chat_id.as_str().to_owned(),
    }
}

fn live_epochs(state: &crate::state::AppState, owner: &RuntimeOwner) -> i64 {
    state
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM session_runtime_epochs WHERE chat_id=?1
               AND state IN ('starting','running','terminating')",
            [owner.chat_id.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}

fn assert_archived_refusal(outcome: Result<(), crate::acp::ipc::AcpIpcError>) {
    assert!(
        matches!(
            &outcome,
            Err(crate::acp::ipc::AcpIpcError::InvalidRequest { message })
                if message.as_str() == "This Chat is archived. Restore it to continue."
        ),
        "{outcome:?}"
    );
}

fn finish_app(state: crate::state::AppState, root: std::path::PathBuf) {
    state.acp.shutdown_blocking().unwrap();
    drop(state);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn archiving_a_chat_after_its_turn_parks_its_runtime_and_it_then_deletes() {
    let (state, owner, root) = app_with_idle_runtime("archive-command");
    assert_eq!(live_epochs(&state, &owner), 1);
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let archived = runtime.block_on(crate::chat_archive_commands::archive_chat_inner(
        &state,
        archive_request(&state, &owner),
        0,
    ));
    assert!(archived.committed, "{:?}", archived.recovery_error);
    assert_eq!(
        live_epochs(&state, &owner),
        1,
        "archive answers before its runtime is released"
    );
    runtime.block_on(crate::chat_archive_commands::release_archived_runtime(
        &state,
        owner.project_id.as_str(),
        owner.chat_id.as_str(),
    ));
    assert_eq!(
        live_epochs(&state, &owner),
        0,
        "an archived Chat holds no runtime"
    );
    assert!(state.acp.lane_for(&owner).is_none());

    let revision = state
        .acp
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    let generation =
        sage_acp_protocol::contract::ProjectGeneration::new(archived.transition_epoch).unwrap();
    let started = runtime.block_on(crate::commands::acp::start_acp_session_inner(
        &state,
        crate::acp::ipc::StartAcpSessionRequest {
            project_id: owner.project_id.clone(),
            project_generation: generation,
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: revision,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new("bindingId", "archive-command-recovery").unwrap(),
            model_id: None,
            effort: None,
        },
    ));
    assert_archived_refusal(started.map(|_| ()));
    let activated = runtime.block_on(crate::commands::acp::activate_acp_session_inner(
        &state,
        crate::acp::ipc::ActivateAcpSessionRequest {
            project_id: owner.project_id.clone(),
            project_generation: generation,
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: revision,
            client_intent_id: ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(owner.runtime_epoch.get() + 1).unwrap(),
        },
    ));
    assert_archived_refusal(activated.map(|_| ()));
    assert_eq!(live_epochs(&state, &owner), 0);

    let deleted = runtime.block_on(crate::chat_archive_commands::delete_chat_inner(
        &state,
        delete_request(&owner),
        archived.transition_epoch,
    ));
    assert!(deleted.committed, "{:?}", deleted.recovery_error);
    finish_app(state, root);
}

#[test]
fn delete_waits_for_a_background_release_still_parking_the_runtime() {
    let (state, owner, root) = app_with_idle_runtime("delete-during-release");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let archived = runtime.block_on(crate::chat_archive_commands::archive_chat_inner(
        &state,
        archive_request(&state, &owner),
        0,
    ));
    assert!(archived.committed, "{:?}", archived.recovery_error);
    state.acp.hold_lane_until_busy_for_test(&owner);
    let deleted = runtime.block_on(crate::chat_archive_commands::delete_chat_inner(
        &state,
        delete_request(&owner),
        archived.transition_epoch,
    ));
    assert_eq!(
        state.acp.lane_busy_refusals_for_test(),
        1,
        "delete waited out the held lane"
    );
    assert!(deleted.committed, "{:?}", deleted.recovery_error);
    assert!(state.acp.lane_for(&owner).is_none());
    assert_eq!(live_epochs(&state, &owner), 0);
    finish_app(state, root);
}

#[test]
fn delete_waits_for_the_harness_to_answer_a_control_then_releases_the_runtime() {
    let pending = Arc::new(Mutex::new(None));
    let engine = TestRuntime {
        pending_native: Some(Arc::clone(&pending)),
        ..TestRuntime::default()
    };
    let (state, owner, root) = app_with_engine("delete-during-control", true, engine);
    state
        .database
        .archive_chat(&archive_request(&state, &owner))
        .unwrap();
    let host = state.acp.host_for(&owner).unwrap();
    let control = {
        let owner = owner.clone();
        std::thread::spawn(move || {
            host.native_rpc(crate::acp::goal::NativeRpcIntent {
                owner,
                request: crate::acp::goal::NativeRpcRequest::UsageRead,
            })
        })
    };
    let started = Instant::now();
    while pending.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }
    let state = Arc::new(state);
    let deleting = {
        let state = Arc::clone(&state);
        let owner = owner.clone();
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new().unwrap().block_on(
                crate::chat_archive_commands::delete_chat_inner(&state, delete_request(&owner), 0),
            )
        })
    };
    while state.acp.control_running_refusals_for_test() == 0 {
        assert!(started.elapsed() < Duration::from_secs(2));
        std::thread::yield_now();
    }
    let answer = pending.lock().unwrap().take().unwrap();
    answer.complete(Ok(crate::acp::goal::NativeRpcReply::UsageRead));
    assert!(control.join().unwrap().is_ok());
    let deleted = deleting.join().unwrap();
    assert!(deleted.committed, "{:?}", deleted.recovery_error);
    assert!(state.acp.lane_for(&owner).is_none());
    let state = Arc::into_inner(state).expect("the delete let go of the app");
    finish_app(state, root);
}

#[test]
fn deleting_an_archived_chat_releases_a_runtime_it_still_holds() {
    let (state, owner, root) = app_with_idle_runtime("delete-command");
    state
        .database
        .archive_chat(&archive_request(&state, &owner))
        .unwrap();
    assert_eq!(live_epochs(&state, &owner), 1);
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let deleted = runtime.block_on(crate::chat_archive_commands::delete_chat_inner(
        &state,
        delete_request(&owner),
        0,
    ));
    assert!(deleted.committed, "{:?}", deleted.recovery_error);
    assert!(state.acp.lane_for(&owner).is_none());
    finish_app(state, root);
}

#[test]
fn archive_refuses_a_running_turn_and_an_archived_chat_takes_no_prompt() {
    let (state, owner, root) = app_with_idle_runtime("archive-refusal");
    let mut prompt = prepare_turn(&owner);
    prompt.expected_projection_revision = state
        .acp
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    state.database.acp_prepare_turn(prompt).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let refused = runtime.block_on(crate::chat_archive_commands::archive_chat_inner(
        &state,
        archive_request(&state, &owner),
        0,
    ));
    assert!(!refused.committed);
    assert_eq!(
        refused.recovery_error.as_deref(),
        Some("Stop this Chat's running turn before archiving it")
    );
    assert_eq!(
        live_epochs(&state, &owner),
        1,
        "a refused archive keeps the runtime"
    );

    finish_app(state, root);

    let (state, owner, root) = app_with_idle_runtime("archived-prompt");
    state
        .database
        .archive_chat(&archive_request(&state, &owner))
        .unwrap();
    let page = state
        .acp
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap();
    let refused = runtime.block_on(crate::commands::acp::send_acp_turn_inner(
        &state,
        crate::acp::ipc::SendAcpTurnRequest::Session {
            project_id: owner.project_id.clone(),
            project_generation: sage_acp_protocol::contract::ProjectGeneration::ZERO,
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            prompt: crate::acp::transcript_quote::Prompt::new("prompt", "archived").unwrap(),
            attached_context: Default::default(),
            staged: None,
        },
    ));
    let failure = refused.expect_err("an archived Chat takes no prompt");
    assert_eq!(
        failure.prompt_dispatch,
        crate::acp::ipc::PromptDispatch::NotStarted
    );
    assert!(
        matches!(
            &failure.failure,
            crate::acp::ipc::AcpIpcError::InvalidRequest { message }
                if message.as_str() == "This Chat is archived. Restore it to continue."
        ),
        "{:?}",
        failure.failure
    );
    assert!(
        state
            .acp
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap()
            .turns
            .as_slice()
            .is_empty(),
        "the refused prompt minted no Turn"
    );
    finish_app(state, root);
}

#[test]
fn releasing_a_chat_parks_only_its_idle_runtime_and_frees_the_lane() {
    let (service, owner) = tauri::async_runtime::block_on(ready_runtime_fixture("release-chat"));
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::default(),
            Arc::new(SuccessfulShutdownRecovery(owner.runtime_epoch.get())),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);
    let other = SageChatId::parse("chat-other").unwrap();

    assert_eq!(
        service.release_chat_runtime(
            &owner.project_id,
            &other,
            crate::acp::service::explicit_close_deadline(),
            || true
        ),
        Ok(())
    );
    assert!(service.lane_at(0).scheduler.active().is_some());

    let layout = |snapshot: sage_domain::project::BootstrapSnapshot| {
        sage_domain::chat::ProjectLayoutRequest {
            project_id: owner.project_id.as_str().to_owned(),
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        }
    };
    service
        .database
        .create_chat(&layout(service.database.bootstrap_snapshot().unwrap()))
        .unwrap();
    service
        .database
        .archive_chat(&sage_domain::chat::ChatLifecycleRequest {
            project_id: owner.project_id.as_str().to_owned(),
            chat_id: owner.chat_id.as_str().to_owned(),
            current_layout: layout(service.database.bootstrap_snapshot().unwrap()),
        })
        .unwrap();
    let delete = sage_domain::chat::ChatDeleteRequest {
        project_id: owner.project_id.as_str().to_owned(),
        chat_id: owner.chat_id.as_str().to_owned(),
    };
    let refused = service.database.delete_archived_chat(&delete).unwrap_err();
    assert!(
        refused.public_message().contains("shutting down"),
        "{refused}"
    );

    assert_eq!(
        service.release_chat_runtime(
            &owner.project_id,
            &owner.chat_id,
            crate::acp::service::explicit_close_deadline(),
            || false,
        ),
        Ok(())
    );
    assert!(service.lane_at(0).scheduler.active().is_some());
    drop(service.claim_lane_activation_for_test(&owner));

    assert_eq!(
        service.release_chat_runtime(
            &owner.project_id,
            &owner.chat_id,
            crate::acp::service::explicit_close_deadline(),
            || true
        ),
        Ok(())
    );
    assert!(service.lane_at(0).scheduler.active().is_none());
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(service.lane_for(&owner).is_none());
    let live: bool = service
        .database
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM session_runtime_epochs WHERE chat_id=?1
               AND state IN ('starting','running','terminating'))",
            [owner.chat_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!live, "the parked runtime epoch is no longer live");
    assert_eq!(
        service.release_chat_runtime(
            &owner.project_id,
            &owner.chat_id,
            crate::acp::service::explicit_close_deadline(),
            || true
        ),
        Ok(())
    );
    service.database.delete_archived_chat(&delete).unwrap();
    assert!(
        service
            .database
            .bootstrap_snapshot()
            .unwrap()
            .archived_chats
            .is_empty()
    );
    service.shutdown_blocking().unwrap();
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
    assert_eq!(
        session.state,
        sage_acp_protocol::lifecycle::SessionState::Failed
    );
    assert_eq!(
        session.residency,
        sage_acp_protocol::lifecycle::Residency::Cold
    );
}

fn prepare_turn(owner: &RuntimeOwner) -> crate::acp::store::PrepareTurnCommand {
    crate::acp::store::PrepareTurnCommand {
        project_id: owner.project_id.clone(),
        project_generation: sage_acp_protocol::contract::ProjectGeneration::new(1).unwrap(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        expected_projection_revision: sage_acp_protocol::contract::ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::new(),
        prompt: crate::acp::transcript_quote::Prompt::new("prompt", "no replay").unwrap(),
        now_millis: sage_core::time::now_millis(),
    }
}

#[derive(Default)]
struct TestRuntime {
    script: Option<(OperationFailure, Arc<std::sync::Mutex<OperationTrace>>)>,
    blocking_close: Option<BlockingCloseRecovery>,
    blocking_prepare: Option<BlockingPrepareRecovery>,
    pending_steer: Option<Arc<Mutex<Option<crate::acp::runtime::SteerCompletionSink>>>>,
    steer_attempts: Option<Arc<AtomicUsize>>,
    pending_native: Option<Arc<Mutex<Option<crate::acp::runtime::NativeRpcCompletionSink>>>>,
}

impl TestRuntime {
    fn scripted(failure: OperationFailure, trace: Arc<std::sync::Mutex<OperationTrace>>) -> Self {
        Self {
            script: Some((failure, trace)),
            blocking_close: None,
            blocking_prepare: None,
            pending_steer: None,
            steer_attempts: None,
            pending_native: None,
        }
    }

    fn blocking_close(entered: SyncSender<()>, release: Receiver<()>) -> Self {
        Self {
            script: None,
            blocking_close: Some(BlockingCloseRecovery {
                attempts: 0,
                entered,
                release: Mutex::new(release),
            }),
            blocking_prepare: None,
            pending_steer: None,
            steer_attempts: None,
            pending_native: None,
        }
    }

    fn blocking_prepare(entered: SyncSender<()>, release: Receiver<()>) -> Self {
        Self {
            script: None,
            blocking_close: None,
            blocking_prepare: Some(BlockingPrepareRecovery {
                entered,
                release: Mutex::new(release),
            }),
            pending_steer: None,
            steer_attempts: None,
            pending_native: None,
        }
    }

    fn delayed_first_steer(
        pending: Arc<Mutex<Option<crate::acp::runtime::SteerCompletionSink>>>,
        attempts: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            script: Some((
                OperationFailure::Cancel,
                Arc::new(Mutex::new(OperationTrace::default())),
            )),
            pending_steer: Some(pending),
            steer_attempts: Some(attempts),
            ..Self::default()
        }
    }
}

fn test_capabilities() -> sage_acp_protocol::intent::CapabilitySnapshotV1 {
    sage_acp_protocol::intent::CapabilitySnapshotV1 {
        schema_version: sage_acp_protocol::contract::SchemaVersionV1,
        protocol_version: sage_acp_protocol::contract::RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        native_session_compact: false,
        native_session_import: false,
        native_review: false,
        native_background_terminals: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: false,
        prompt_images: false,
        config_count: 0,
    }
}

impl crate::acp::runtime::RuntimeEngine for TestRuntime {
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn begin_native_rpc(
        &mut self,
        _: &crate::acp::goal::NativeRpcIntent,
        _: Instant,
        completion: crate::acp::runtime::NativeRpcCompletionSink,
    ) -> Result<(), crate::acp::runtime::HostError> {
        match &self.pending_native {
            Some(pending) => *pending.lock().unwrap() = Some(completion),
            None => completion.complete(Err(crate::acp::runtime::HostError::State)),
        }
        Ok(())
    }

    fn begin_steer(
        &mut self,
        _: &crate::acp::runtime::HostSteerIntent,
        _: Instant,
        completion: crate::acp::runtime::SteerCompletionSink,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let Some(attempts) = &self.steer_attempts else {
            return Err(crate::acp::runtime::HostError::State);
        };
        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            *self.pending_steer.as_ref().unwrap().lock().unwrap() = Some(completion);
        } else {
            completion.complete(Ok(()));
        }
        Ok(())
    }

    fn prepare_prompt(
        &mut self,
        _: &crate::acp::runtime::HostPromptIntent,
    ) -> Result<sage_acp_protocol::contract::SageTurnId, crate::acp::runtime::HostError> {
        if let Some(recovery) = &self.blocking_prepare {
            recovery
                .entered
                .send(())
                .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
            recovery
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(1))
                .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
            return sage_acp_protocol::contract::SageTurnId::parse("turn-timeout-recovery")
                .map_err(|_| crate::acp::runtime::HostError::Intent);
        }
        let Some((failure, trace)) = &self.script else {
            return Err(crate::acp::runtime::HostError::Unavailable);
        };
        trace.lock().unwrap().send += 1;
        if matches!(failure, OperationFailure::Send) {
            return Err(crate::acp::runtime::HostError::RecoveryRequired);
        }
        sage_acp_protocol::contract::SageTurnId::parse("turn-operation-recovery")
            .map_err(|_| crate::acp::runtime::HostError::Intent)
    }

    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn: &sage_acp_protocol::contract::SageTurnId,
    ) -> Result<crate::acp::runtime::PromptWrite, crate::acp::runtime::HostError> {
        if matches!(self.script, Some((OperationFailure::SignIn, _))) {
            return Err(crate::acp::runtime::HostError::SignInRequired);
        }
        self.script
            .as_ref()
            .map(|_| crate::acp::runtime::PromptWrite::Complete(turn.clone()))
            .ok_or(crate::acp::runtime::HostError::Unavailable)
    }

    fn retry_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &sage_acp_protocol::contract::SageTurnId,
    ) -> Result<crate::acp::runtime::PromptWrite, crate::acp::runtime::HostError> {
        Err(crate::acp::runtime::HostError::Unavailable)
    }

    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &sage_acp_protocol::contract::SageTurnId,
    ) -> Result<(), crate::acp::runtime::HostError> {
        Ok(())
    }

    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &sage_acp_protocol::contract::SageTurnId,
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
        if let Some(recovery) = self.blocking_close.as_mut() {
            recovery.attempts += 1;
            return (recovery.attempts > 1)
                .then_some(())
                .ok_or(crate::acp::runtime::HostError::RecoveryRequired);
        }
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
        if let Some(recovery) = &self.blocking_close {
            recovery
                .entered
                .send(())
                .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
            recovery
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(1))
                .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
        }
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

fn insert_sending_turn(service: &AcpService, owner: &RuntimeOwner) {
    service
        .database
        .connection()
        .execute(
            "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
                 state,delivery_state,revision,created_at,updated_at,terminal_at)
             VALUES('turn-operation-recovery',?1,?2,?3,?4,0,'intent-sign-in','sending','notWritten',0,1,1,NULL)",
            rusqlite::params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64
            ],
        )
        .unwrap();
}

fn turn_state(
    service: &AcpService,
    owner: &RuntimeOwner,
) -> sage_acp_protocol::lifecycle::TurnState {
    service
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap()
        .turns
        .as_slice()
        .iter()
        .find(|turn| turn.id.as_str() == "turn-operation-recovery")
        .unwrap()
        .state
}

#[test]
fn a_refused_sign_in_prompt_settles_its_turn_and_frees_the_lane() {
    let (service, owner, trace) = service_with_operation_recovery(OperationFailure::SignIn);
    insert_sending_turn(&service, &owner);
    let failure = service
        .send_session_turn(crate::acp::runtime::HostPromptIntent {
            command: prepare_turn(&owner),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap_err();
    assert!(matches!(failure.error, CodexSessionError::SignInRequired));
    assert_eq!(trace.lock().unwrap().send, 1);
    assert!(turn_state(&service, &owner).is_terminal());
    assert!(service.lane_for(&owner).is_none(), "the lane is free again");
    assert_recovered_runtime_is_cold(&service, &owner);
    assert!(matches!(
        service.cancel_codex_turn(cancel_intent(&owner)),
        Ok(crate::acp::runtime::HostOutcome::CancelQueued)
    ));
    service.shutdown_blocking().unwrap();
}

#[test]
fn cancel_settles_a_turn_whose_host_failed_closed() {
    let (service, owner, _trace) = service_with_operation_recovery(OperationFailure::SignIn);
    insert_sending_turn(&service, &owner);
    let host = service.host_for(&owner).unwrap();
    // Drive the host directly so only Cancel can settle the wedged turn.
    assert_eq!(
        host.send(crate::acp::runtime::HostPromptIntent {
            command: prepare_turn(&owner),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        }),
        Err(crate::acp::prompt_send::PromptSendError::unknown(
            crate::acp::runtime::HostError::SignInRequired
        ))
    );
    assert!(matches!(
        host.cancel(cancel_intent(&owner)),
        Err(crate::acp::runtime::HostError::Intent)
    ));
    assert!(matches!(
        service.cancel_codex_turn(cancel_intent(&owner)),
        Ok(crate::acp::runtime::HostOutcome::CancelQueued)
    ));
    assert!(turn_state(&service, &owner).is_terminal());
    assert!(service.lane_for(&owner).is_none());
    service.shutdown_blocking().unwrap();
}

fn cancel_intent(owner: &RuntimeOwner) -> crate::acp::runtime::HostCancelIntent {
    crate::acp::runtime::HostCancelIntent {
        owner: owner.clone(),
        turn_id: sage_acp_protocol::contract::SageTurnId::parse("turn-operation-recovery").unwrap(),
        client_intent_id: ClientIntentId::new(),
        now_millis: sage_core::time::now_millis(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn refused_prompt_recovery_allows_other_async_work_to_progress() {
    let (service, owner) = ready_runtime_fixture("refused-progress").await;
    let trace = Arc::new(Mutex::new(OperationTrace::default()));
    let (entered, waiting) = std::sync::mpsc::sync_channel(1);
    let (release, released) = std::sync::mpsc::sync_channel(1);
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            TestRuntime::scripted(OperationFailure::SignIn, trace),
            Arc::new(BlockingShutdownRecovery {
                entered,
                release: Mutex::new(released),
                epoch: owner.runtime_epoch.get(),
            }),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);
    insert_sending_turn(&service, &owner);
    let progress = tokio::spawn(async move {
        loop {
            if waiting.try_recv().is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        release.send(()).unwrap();
    });
    let failure = service
        .send_session_turn(crate::acp::runtime::HostPromptIntent {
            command: prepare_turn(&owner),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap_err();
    progress.await.unwrap();
    assert!(matches!(failure.error, CodexSessionError::SignInRequired));
    assert!(turn_state(&service, &owner).is_terminal());
    assert!(service.lane_for(&owner).is_none());
    service.shutdown_blocking().unwrap();
}

fn complete_a_turn(service: &AcpService, owner: &RuntimeOwner) {
    service
        .database
        .connection()
        .execute(
            "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
                 state,delivery_state,revision,created_at,updated_at,terminal_at)
             VALUES('turn-idle-release',?1,?2,?3,?4,0,'intent-idle-release','completed','correlated',0,1,1,1)",
            rusqlite::params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                owner.runtime_epoch.get() as i64
            ],
        )
        .unwrap();
}

fn runtime_epoch_is_live(service: &AcpService, owner: &RuntimeOwner) -> bool {
    service
        .database
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM session_runtime_epochs WHERE chat_id=?1
               AND state IN ('starting','running','terminating'))",
            [owner.chat_id.as_str()],
            |row| row.get(0),
        )
        .unwrap()
}

fn idle_release_fixture(label: &str) -> (AcpService, RuntimeOwner) {
    idle_runtime_fixture(label, true, true)
}

fn idle_runtime_fixture(
    label: &str,
    resumable: bool,
    completed_turn: bool,
) -> (AcpService, RuntimeOwner) {
    idle_runtime_fixture_with(label, resumable, completed_turn, TestRuntime::default())
}

fn idle_runtime_fixture_with(
    label: &str,
    resumable: bool,
    completed_turn: bool,
    runtime: TestRuntime,
) -> (AcpService, RuntimeOwner) {
    let capabilities = sage_acp_protocol::intent::CapabilitySnapshotV1 {
        resume_session: resumable,
        ..test_capabilities()
    };
    let (service, owner) =
        tauri::async_runtime::block_on(ready_runtime_fixture_with(label, capabilities));
    if completed_turn {
        complete_a_turn(&service, &owner);
    }
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            runtime,
            Arc::new(SuccessfulShutdownRecovery(owner.runtime_epoch.get())),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);
    (service, owner)
}

const MINUTE: Duration = Duration::from_secs(60);

#[test]
fn an_idle_chat_out_of_view_releases_its_runtime_after_two_minutes() {
    let (service, owner) = idle_release_fixture("idle-hidden");
    let start = std::time::Instant::now();

    service.set_shown_chats([owner.chat_id.clone()]);
    assert_eq!(service.release_idle_runtimes(start + 3 * MINUTE), Some(0));
    assert!(service.lane_for(&owner).is_some());

    service.set_shown_chats([]);
    assert_eq!(service.release_idle_runtimes(start + MINUTE), Some(0));
    assert!(service.lane_for(&owner).is_some());
    assert_eq!(
        service.release_idle_runtimes(start + 2 * MINUTE + Duration::from_secs(1)),
        Some(1)
    );
    assert!(service.lane_for(&owner).is_none());
    assert!(service.lane_at(0).codex_recovery.lock().unwrap().is_none());
    assert!(
        !runtime_epoch_is_live(&service, &owner),
        "the parked runtime epoch is no longer live"
    );
    assert!(
        service.lane_at(0).scheduler.active().is_none(),
        "the lane is free for the next activation"
    );
    service.shutdown_blocking().unwrap();
}

#[test]
fn an_idle_chat_in_view_is_released_only_after_fifteen_minutes() {
    let (service, owner) = idle_release_fixture("idle-shown");
    let start = std::time::Instant::now();
    service.set_shown_chats([owner.chat_id.clone()]);
    assert_eq!(service.release_idle_runtimes(start + 14 * MINUTE), Some(0));
    assert!(service.lane_for(&owner).is_some());
    assert_eq!(service.release_idle_runtimes(start + 15 * MINUTE), Some(1));
    assert!(service.lane_for(&owner).is_none());
    assert!(!runtime_epoch_is_live(&service, &owner));
    service.shutdown_blocking().unwrap();
}

#[test]
fn before_any_view_report_only_the_fifteen_minute_rule_applies() {
    let (service, owner) = idle_release_fixture("idle-unreported");
    let start = std::time::Instant::now();
    assert_eq!(service.release_idle_runtimes(start + 5 * MINUTE), Some(0));
    assert_eq!(service.release_idle_runtimes(start + 10 * MINUTE), Some(0));
    assert!(service.lane_for(&owner).is_some());
    assert_eq!(service.release_idle_runtimes(start + 15 * MINUTE), Some(1));
    assert!(service.lane_for(&owner).is_none());
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_lane_held_by_an_activation_keeps_an_idle_chat_runtime() {
    let (service, owner) = idle_release_fixture("idle-held");
    let start = std::time::Instant::now();
    service.set_shown_chats([]);
    {
        let _claim = service.claim_lane_activation_for_test(&owner);
        assert_eq!(service.release_idle_runtimes(start + 20 * MINUTE), Some(0));
        assert!(service.lane_for(&owner).is_some());
    }
    drop(service.claim_lane_activation_for_test(&owner));
    assert_eq!(service.release_idle_runtimes(start + 20 * MINUTE), Some(1));
    assert!(service.lane_for(&owner).is_none());
    service.shutdown_blocking().unwrap();
}

#[test]
fn an_idle_chat_whose_harness_is_answering_a_control_keeps_its_runtime() {
    let pending = Arc::new(Mutex::new(None));
    let runtime = TestRuntime {
        pending_native: Some(Arc::clone(&pending)),
        ..TestRuntime::default()
    };
    let (service, owner) = idle_runtime_fixture_with("idle-control", true, true, runtime);
    let start = std::time::Instant::now();
    service.set_shown_chats([]);
    let host = service.host_for(&owner).unwrap();
    let fork = {
        let owner = owner.clone();
        std::thread::spawn(move || {
            host.native_rpc(crate::acp::goal::NativeRpcIntent {
                owner,
                request: crate::acp::goal::NativeRpcRequest::ForkSession,
            })
        })
    };
    let waiting = std::time::Instant::now();
    while pending.lock().unwrap().is_none() {
        assert!(waiting.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }
    assert_eq!(service.release_idle_runtimes(start + 20 * MINUTE), Some(0));
    assert!(service.lane_for(&owner).is_some());
    let answer = pending.lock().unwrap().take().unwrap();
    answer.complete(Ok(crate::acp::goal::NativeRpcReply::Rejected));
    assert_eq!(
        fork.join().unwrap(),
        Ok(crate::acp::goal::NativeRpcReply::Rejected),
        "the control was answered, not cut off"
    );
    assert_eq!(service.release_idle_runtimes(start + 20 * MINUTE), Some(1));
    assert!(service.lane_for(&owner).is_none());
    service.shutdown_blocking().unwrap();
}

#[test]
fn an_eviction_park_takes_the_claim_made_when_its_lane_was_chosen() {
    let (service, owner) = idle_release_fixture("evict-claimed");
    let lane = service.lane_for(&owner).unwrap();
    let host = lane.runtime.for_owner(&owner).unwrap();
    *lane.eviction.lock().unwrap() = Some(host.claim_idle_park().unwrap());
    assert!(host.claim_idle_park().is_none());
    let deadline = crate::acp::service::explicit_close_deadline();
    let parked = lane.park_idle_runtime_for_replacement(deadline);
    assert!(
        matches!(parked, Err(CodexSessionError::Recovery)),
        "the park went ahead and left its zero-process recovery: {parked:?}"
    );
    assert!(
        lane.eviction.lock().unwrap().is_none(),
        "the park took the claim"
    );
    assert!(lane.has_replacement_park_recovery());
    assert_eq!(lane.shutdown_codex_recovery(deadline), Ok(true));
    assert!(service.lane_for(&owner).is_none());
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_chat_shown_again_is_kept_and_sweeping_ends_at_shutdown() {
    let (service, owner) = idle_release_fixture("idle-returned");
    let start = std::time::Instant::now();
    service.set_shown_chats([]);
    assert_eq!(service.release_idle_runtimes(start + MINUTE), Some(0));
    service.set_shown_chats([owner.chat_id.clone()]);
    assert_eq!(service.release_idle_runtimes(start + 5 * MINUTE), Some(0));
    assert!(service.lane_for(&owner).is_some());

    service.shutdown_blocking().unwrap();
    assert_eq!(service.release_idle_runtimes(start + 20 * MINUTE), None);
}

#[test]
fn a_session_that_cannot_resume_or_never_completed_a_turn_is_kept() {
    for (label, resumable, completed_turn) in [
        ("idle-unresumable", false, true),
        ("idle-unused", true, false),
    ] {
        let (service, owner) = idle_runtime_fixture(label, resumable, completed_turn);
        let start = std::time::Instant::now();
        service.set_shown_chats([]);
        assert_eq!(service.release_idle_runtimes(start + MINUTE), Some(0));
        // Parking would lose the conversation, or nothing proves it resumes.
        assert_eq!(
            service.release_idle_runtimes(start + 30 * MINUTE),
            Some(0),
            "{label}"
        );
        assert!(service.lane_for(&owner).is_some(), "{label}");
        assert!(runtime_epoch_is_live(&service, &owner), "{label}");
        service.shutdown_blocking().unwrap();
    }
}

#[test]
fn a_chat_shown_again_while_a_sweep_runs_is_kept() {
    let (service, owner) = idle_release_fixture("idle-came-back");
    let start = std::time::Instant::now();
    service.set_shown_chats([]);
    assert_eq!(service.release_idle_runtimes(start + MINUTE), Some(0));
    let now = start + 3 * MINUTE;
    let due = service.idle_runtimes_due(now);
    assert_eq!(due.len(), 1);
    service.set_shown_chats([owner.chat_id.clone()]);
    let (index, due_owner, idle_since) = due.into_iter().next().unwrap();
    let deadline = crate::acp::service::explicit_close_deadline();
    assert!(!service.park_idle_lane(index, &due_owner, idle_since, now, deadline));
    assert!(service.lane_for(&owner).is_some());
    assert!(runtime_epoch_is_live(&service, &owner));
    service.shutdown_blocking().unwrap();
}
