use super::*;
use crate::acp::{
    contract::{EntityRevision, ProjectionRevision, RuntimeEpoch, SafeText, SageAgentSessionId},
    intent::CapabilitySnapshotV1,
    projection::SessionProjection,
    runtime::{
        HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent, HostState, PromptWrite,
        RuntimeEngine, RuntimeHost,
    },
    service::runtime_slot::RuntimeSlotError,
    session_config::{ConfigId, ConfigMutation, ConfigValue, SessionConfig},
    store::AdapterTurnCursor,
};
use serde_json::json;

struct ConfigErrorEngine(HostError);

impl RuntimeEngine for ConfigErrorEngine {
    fn begin_configure_session(
        &mut self,
        _: &ConfigIntent,
        _: std::time::Instant,
        completion: crate::acp::runtime::ConfigCompletionSink,
    ) -> Result<(), HostError> {
        completion.complete(Err(self.0.clone()));
        Ok(())
    }
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        Ok(())
    }
    fn prepare_prompt(
        &mut self,
        _: &HostPromptIntent,
    ) -> Result<crate::acp::contract::SageTurnId, HostError> {
        Err(HostError::State)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn retry_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
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
    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
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
    fn shutdown(&mut self, _: std::time::Instant) -> Result<(), HostError> {
        Ok(())
    }
}

fn fixture(error: HostError) -> (AcpService, ConfigIntent, AcpChatPage) {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    let owner = RuntimeOwner {
        project_id: SageProjectId::parse("project-config").unwrap(),
        chat_id: SageChatId::parse("chat-config").unwrap(),
        session_id: SageAgentSessionId::parse("session-config").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    let host = Arc::new(RuntimeHost::start(ConfigErrorEngine(error)).unwrap());
    host.activate(owner.clone()).unwrap();
    service.runtime.reserve_start(owner.clone()).unwrap();
    service
        .runtime
        .config
        .update(
            &owner,
            SessionConfig::from_wire(
                &json!({"configOptions": [{
                    "id": "effort", "name": "Effort", "type": "select",
                    "currentValue": "high", "options": [{"value": "high", "name": "High"}]
                }]}),
                true,
            )
            .unwrap(),
        )
        .unwrap();
    service.runtime.finish_start(&owner, host).unwrap();
    service.runtime.publish_start(&owner).unwrap();
    let intent = ConfigIntent {
        expected_config_revision: service
            .runtime
            .config
            .snapshot(&owner)
            .unwrap()
            .config_revision,
        owner: owner.clone(),
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("id", "effort").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", "high").unwrap(),
            },
        },
    };
    let capabilities: CapabilitySnapshotV1 = serde_json::from_value(json!({
        "schemaVersion": 1, "protocolVersion": "acp-1",
        "newSession": true, "resumeSession": true, "loadSession": false,
        "closeSession": true, "prompt": true, "cancel": true,
        "nativeSessionFork": false, "activeTurnSteer": false,
        "childConversationProjection": false, "childTranscriptRead": false,
        "collaborationEventProjection": false, "multiRootExtensionSchemaId": null,
        "modelCount": 0, "configCount": 0
    }))
    .unwrap();
    let mut page = AcpChatPage {
        session_config: None,
        goal: crate::acp::goal::GoalState::Unsupported,
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        title: SafeText::new("title", "Config").unwrap(),
        projection_revision: ProjectionRevision::ZERO,
        current_session_id: Some(owner.session_id.clone()),
        sessions: BoundedVec::new(
            "sessions",
            vec![SessionProjection {
                id: owner.session_id.clone(),
                ordinal: 0,
                binding_id: BindingId::new("bindingId", "binding-config").unwrap(),
                model_id: None,
                state: crate::acp::contract::SessionState::Ready,
                residency: crate::acp::contract::Residency::Hot,
                runtime_epoch: Some(owner.runtime_epoch),
                next_runtime_epoch: None,
                permission_requests: false,
                prompt_content: Default::default(),
                capabilities,
                revision: EntityRevision::ZERO,
                projection_revision: ProjectionRevision::ZERO,
                legacy: false,
            }],
        )
        .unwrap(),
        turns: BoundedVec::default(),
        next_cursor: None,
    };
    service.compose_session_config(&mut page).unwrap();
    assert!(page.session_config.is_some());
    assert_eq!(page.sessions.as_slice()[0].capabilities.config_count, 1);
    (service, intent, page)
}

#[test]
fn fatal_config_transport_errors_recover_exact_slot_and_clear_live_overlay() {
    for error in [
        HostError::Unavailable,
        HostError::Closed,
        HostError::Deadline,
        HostError::DeliveryUnknown,
        HostError::RecoveryRequired,
    ] {
        let (service, intent, mut page) = fixture(error.clone());
        assert_eq!(service.configure_session(intent.clone()), Err(error));
        assert_eq!(
            service.runtime.active().unwrap().state().unwrap(),
            HostState::Recovery
        );
        assert!(matches!(
            service.runtime.for_owner(&intent.owner),
            Err(RuntimeSlotError::Recovery)
        ));
        let tombstone = service.runtime.config.snapshot(&intent.owner).unwrap();
        assert!(tombstone.removed);
        assert_eq!(tombstone.config_revision.get(), 2);
        // Even an otherwise hot durable page must expose the exact-owner
        // tombstone so a delayed page cannot restore stale controls.
        service.compose_session_config(&mut page).unwrap();
        assert!(
            page.session_config
                .as_ref()
                .is_some_and(|config| config.removed)
        );
        assert_eq!(page.sessions.as_slice()[0].capabilities.config_count, 0);
        service.shutdown_blocking().unwrap();
    }
}

#[test]
fn nonfatal_config_errors_keep_claimed_slot_and_live_overlay() {
    for error in [
        HostError::State,
        HostError::Intent,
        HostError::Capacity,
        HostError::RetryExhausted,
        HostError::ConfigMutationRejected,
    ] {
        let (service, intent, mut page) = fixture(error.clone());
        let overlay = page.session_config.clone();
        assert_eq!(service.configure_session(intent.clone()), Err(error));
        assert!(service.runtime.for_owner(&intent.owner).is_ok());
        assert_eq!(service.runtime.config.snapshot(&intent.owner), overlay);
        service.compose_session_config(&mut page).unwrap();
        assert_eq!(page.session_config, overlay);
        assert_eq!(page.sessions.as_slice()[0].capabilities.config_count, 1);
        service.shutdown_blocking().unwrap();
    }
}

#[test]
fn stale_config_owner_does_not_recover_the_current_owner() {
    let (service, mut intent, mut page) = fixture(HostError::Unavailable);
    let owner = intent.owner.clone();
    let overlay = page.session_config.clone();
    intent.owner.runtime_epoch = RuntimeEpoch::new(2).unwrap();
    assert_eq!(service.configure_session(intent), Err(HostError::Intent));
    assert!(service.runtime.for_owner(&owner).is_ok());
    service.compose_session_config(&mut page).unwrap();
    assert_eq!(page.session_config, overlay);
    assert_eq!(page.sessions.as_slice()[0].capabilities.config_count, 1);
    service.shutdown_blocking().unwrap();
}

#[test]
fn fatal_config_transport_errors_retain_recovery_that_releases_capacity() {
    for error in [
        HostError::Unavailable,
        HostError::Closed,
        HostError::Deadline,
        HostError::DeliveryUnknown,
        HostError::RecoveryRequired,
    ] {
        let (service, intent, _) = fixture(error.clone());
        assert_eq!(service.configure_session(intent.clone()), Err(error));
        assert!(matches!(
            service.runtime.for_owner(&intent.owner),
            Err(RuntimeSlotError::Recovery)
        ));

        assert!(
            service
                .reclaim_zero_process_capacity(
                    std::time::Instant::now() + std::time::Duration::from_secs(1)
                )
                .unwrap()
        );
        let next = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(intent.owner.runtime_epoch.get() + 1).unwrap(),
            ..intent.owner
        };
        service.runtime.reserve_start(next.clone()).unwrap();
        service.runtime.abandon_start(&next).unwrap();
        service.shutdown_blocking().unwrap();
    }
}
