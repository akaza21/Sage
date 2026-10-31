use super::*;
use crate::acp::{
    bindings::registry::BindingProjectRoot, context_handoff::HandoffState,
    runtime::HostPromptIntent, store::session_replace::ReplaceSessionCommand,
};

impl AcpService {
    /// Shared by the named handoff fixture test; all runtime authority belongs
    /// to this service's production lanes, including the transition shutdown.
    pub(crate) async fn assert_handoff_preserves_other_live_lane() {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "codex-journey-turn-success",
                "characterization-two-turn-success",
            ]))
            .await;
        let first = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let other = setup
            .start_session_for_chat(setup.create_chat(), RuntimeEpoch::new(1).unwrap())
            .await;
        setup.activate(&other).await.unwrap();
        setup.wait_for_ready(&other);
        setup.send_and_wait_completed(&first).await;
        let other_first_turn = setup.send_and_wait_completed(&other).await;
        let before = setup.page(&other);
        let other_session = before
            .sessions
            .as_slice()
            .iter()
            .find(|session| session.id == other.session_id)
            .unwrap()
            .clone();
        let host = setup.service.host_for(&other).unwrap();
        let scheduler = setup.service.lane_for(&other).unwrap().scheduler;
        assert_eq!(scheduler.active().unwrap().tree, other);
        // The diagnostic ledger is single-slot: the first published lane owns it
        // and later lanes cannot overwrite it.
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        assert_eq!(
            setup
                .service
                .codex_runtime_identity()
                .expect("first native process identity")
                .owner
                .session_id,
            first.session_id.as_str()
        );

        // A newly selected admitted model is explicitly classified fresh.
        let model = crate::acp::intent::ModelId::new("modelId", "fixture-model").unwrap();
        crate::acp::service::tests::seed_codex_discovery(
            &setup.service,
            &setup.root(),
            Some(model.clone()),
        );
        let mut start =
            setup.start_command_for(&first.chat_id, setup.page(&first).projection_revision);
        start.model_id = Some(model);
        setup
            .service
            .transition_session(
                setup
                    .service
                    .binding_registry()
                    .resolve("codex-app-server")
                    .unwrap(),
                &BindingProjectRoot::Codex(setup.root()),
                ReplaceSessionCommand {
                    expected_head: Some(first.session_id.clone()),
                    confirm_unknown_delivery: false,
                    start,
                },
            )
            .unwrap();
        let replaced = setup.page(&first);
        assert_ne!(replaced.current_session_id, Some(first.session_id.clone()));
        assert_eq!(
            replaced.handoffs.last().unwrap().state,
            HandoffState::Prepared
        );
        assert!(setup.service.lane_for(&first).is_none());

        let after = setup.page(&other);
        assert_eq!(after.current_session_id, before.current_session_id);
        assert_eq!(after.projection_revision, before.projection_revision);
        assert_eq!(
            after
                .sessions
                .as_slice()
                .iter()
                .find(|session| session.id == other.session_id)
                .unwrap(),
            &other_session
        );
        assert!(Arc::ptr_eq(&host, &setup.service.host_for(&other).unwrap()));
        assert_eq!(scheduler.active().unwrap().tree, other);
        // Only the transitioned owner's diagnostic is cleared; the other lane
        // keeps its own host, scheduler tree and process (proven by its next turn).
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        assert!(setup.service.codex_runtime_identity().is_none());

        setup
            .service
            .send_session_turn(HostPromptIntent {
                command: setup.turn_command(&other),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .unwrap();
        let next = setup.wait_for_completed_after(&other, std::slice::from_ref(&other_first_turn));
        assert_completed_turns(&setup, &other, &[other_first_turn, next]);
        assert!(Arc::ptr_eq(&host, &setup.service.host_for(&other).unwrap()));
        setup.close(&other);
        setup.service.shutdown_blocking().unwrap();
        assert_job_zero();
    }
}
