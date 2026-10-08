use super::*;
use std::time::Duration;

#[test]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn shutdown_running_codex_turn_converges_within_close_budget() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup = FixtureSetup::in_memory(
            CodexBindingRegistration::admitted_for_observed_steer_journey_test(&[
                "codex-steer-lost-result",
            ]),
        )
        .await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        setup
            .service
            .send_codex_turn(setup.turn_command(&owner))
            .unwrap();
        setup.wait_for_running_turn(&owner);
        let host = setup.service.runtime.active().unwrap();
        assert_eq!(
            host.state().unwrap(),
            crate::acp::runtime::HostState::Running
        );
        let started = Instant::now();
        let outcome = setup.service.shutdown_blocking();
        let elapsed = started.elapsed();
        assert_eq!(
            outcome,
            Ok(()),
            "shutdown stage={} elapsed={:?}",
            setup.service.last_shutdown_stage(),
            elapsed
        );
        assert_eq!(host.last_shutdown_phase(), "begin_park_intent");
        assert!(
            elapsed
                <= Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close),
            "shutdown exceeded its close budget: {elapsed:?}"
        );
        assert_job_zero();
    });
}

#[test]
#[cfg(windows)]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn shutdown_running_codex_turn_preserves_partial_chat_after_reopen() {
    let _serial = crate::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::file_backed(CodexBindingRegistration::admitted_for_journey_test(&[
                "codex-partial-turn-at-shutdown",
            ]))
            .await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        setup
            .service
            .send_codex_turn(setup.turn_command(&owner))
            .unwrap();
        setup.wait_until(|| {
            setup.page(&owner).turns.as_slice().iter().any(|turn| {
                turn.session_id == owner.session_id
                    && turn.items.as_slice().iter().any(|item| {
                        matches!(&item.content, ItemContent::Text { role: crate::acp::projection::TextRole::Assistant, text } if text.as_str() == "ok")
                    })
            })
        });
        let database_path = setup.database_path.clone().unwrap();
        setup.service.shutdown_blocking().unwrap();
        drop(setup);
        assert_job_zero();

        let database = crate::persistence::Database::open(&database_path).unwrap();
        let reopened = AcpService::new_with_codex_publisher_for_test(
            database,
            CodexBindingRegistration::unavailable_for_test(),
            std::sync::Arc::new(JourneyPublisher),
        )
        .unwrap();
        let page = reopened
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap();
        assert_eq!(page.current_session_id, None);
        let session = page
            .sessions
            .as_slice()
            .iter()
            .find(|session| session.id == owner.session_id)
            .unwrap();
        assert_eq!(session.binding_id.as_str(), "codex-app-server");
        assert_eq!(session.state, SessionState::Failed);
        let turn = page
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.session_id == owner.session_id)
            .unwrap();
        assert!(matches!(
            turn.state,
            TurnState::Failed | TurnState::Interrupted
        ));
        assert!(turn.items.as_slice().iter().any(|item| {
            matches!(&item.content, ItemContent::Text { role: crate::acp::projection::TextRole::User, text } if text.as_str() == "Complete this short test turn.")
        }));
        assert!(turn.items.as_slice().iter().any(|item| {
            matches!(&item.content, ItemContent::Text { role: crate::acp::projection::TextRole::Assistant, text } if text.as_str() == "ok")
        }));
        reopened.shutdown_blocking().unwrap();
        drop(reopened);
        std::fs::remove_dir_all(database_path.parent().unwrap()).unwrap();
    });
}
