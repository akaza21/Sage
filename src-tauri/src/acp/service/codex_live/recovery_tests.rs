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
                binding_id: BindingId::new("bindingId", "observer-recovery").unwrap(),
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
        let lease = service
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
        let mut lease = lease;
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
                    "observer-external",
                )
                .unwrap(),
                capabilities_json: serde_json::to_string(&test_capabilities()).unwrap(),
                now_millis: crate::persistence::now_millis(),
            })
            .await
            .unwrap();
        drop(lease);

        let running = crate::acp::process::launch_fixture_for_test(owner.runtime_epoch.get());
        let (_, _, control) = running.into_parts();
        let runner = LiveRunnerHandle::new(Arc::new(DriverMailbox::new().unwrap()), control);
        let host = Arc::new(crate::acp::runtime::RuntimeHost::start(TestRuntime).unwrap());
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

struct TestRuntime;

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
        Err(crate::acp::runtime::HostError::Unavailable)
    }

    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<crate::acp::runtime::PromptWrite, crate::acp::runtime::HostError> {
        Err(crate::acp::runtime::HostError::Unavailable)
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
        Ok(())
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
        Ok(())
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
        Ok(())
    }
}
