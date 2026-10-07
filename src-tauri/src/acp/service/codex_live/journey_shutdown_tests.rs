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
