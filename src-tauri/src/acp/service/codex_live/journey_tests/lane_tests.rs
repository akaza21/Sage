use super::*;
use crate::acp::service::LIVE_RUNTIME_LANES;

fn live_trees() -> u32 {
    crate::acp::process::TreeCapacityPool::global()
        .observation_usage()
        .0
}

#[test]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn two_chats_run_concurrently_each_on_its_own_runtime() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-two-turn-success",
                "characterization-two-turn-success",
            ]))
            .await;
        let owner_a = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let owner_b = setup
            .start_session_for_chat(setup.create_chat(), RuntimeEpoch::new(1).unwrap())
            .await;
        setup.activate(&owner_b).await.unwrap();
        setup.wait_for_ready(&owner_b);
        assert_eq!(live_trees(), 2, "each live chat owns one runtime tree");

        let before_a = setup.completed_turns(&owner_a);
        let before_b = setup.completed_turns(&owner_b);
        setup
            .service
            .send_codex_turn(setup.turn_command(&owner_a))
            .unwrap();
        setup
            .service
            .send_codex_turn(setup.turn_command(&owner_b))
            .unwrap();
        let turn_a = setup.wait_for_completed_after(&owner_a, &before_a);
        let turn_b = setup.wait_for_completed_after(&owner_b, &before_b);
        assert_completed_turns(&setup, &owner_a, std::slice::from_ref(&turn_a));
        assert_completed_turns(&setup, &owner_b, std::slice::from_ref(&turn_b));

        let second_a = setup.send_and_wait_completed(&owner_a).await;
        assert_completed_turns(&setup, &owner_a, &[turn_a, second_a]);
        setup.close(&owner_a);
        assert_eq!(live_trees(), 1, "closing one chat leaves the other live");
        let second_b = setup.send_and_wait_completed(&owner_b).await;
        assert_completed_turns(&setup, &owner_b, &[turn_b, second_b]);
        setup.close(&owner_b);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg_attr(
    not(feature = "acp-fixture"),
    ignore = "requires acp-fixture; run cargo test --all-features"
)]
fn full_lanes_park_the_least_recent_idle_runtime_and_a_failed_park_retains_recovery() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        const CASES: [&str; LIVE_RUNTIME_LANES + 1] =
            ["characterization-two-turn-success"; LIVE_RUNTIME_LANES + 1];
        let setup = FixtureSetup::in_memory_with_shutdown_phase_failure(
            CodexBindingRegistration::admitted_for_journey_test(&CASES),
        )
        .await;
        // The first runtime's port fails its park, so it is the LRU victim.
        let first = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let first_turn = setup.send_and_wait_completed(&first).await;
        let mut owners = vec![first.clone()];
        for _ in 1..LIVE_RUNTIME_LANES {
            let owner = setup
                .start_session_for_chat(setup.create_chat(), RuntimeEpoch::new(1).unwrap())
                .await;
            setup.activate(&owner).await.unwrap();
            setup.wait_for_ready(&owner);
            setup.send_and_wait_completed(&owner).await;
            owners.push(owner);
        }
        assert_eq!(live_trees() as usize, LIVE_RUNTIME_LANES);

        let overflow = setup
            .start_session_for_chat(setup.create_chat(), RuntimeEpoch::new(1).unwrap())
            .await;
        assert!(matches!(
            setup.activate(&overflow).await,
            Err(CodexSessionError::Recovery)
        ));
        assert!(live_trees() as usize <= LIVE_RUNTIME_LANES);
        assert!(setup.service.has_retained_live_host_recovery_for(&first));
        assert_completed_turns(&setup, &first, &[first_turn]);

        setup.activate(&overflow).await.unwrap();
        setup.wait_for_ready(&overflow);
        let parked = setup
            .page(&first)
            .sessions
            .as_slice()
            .iter()
            .find(|session| session.id == first.session_id)
            .unwrap()
            .clone();
        assert_eq!(parked.state, SessionState::Ready);
        assert_eq!(parked.residency, crate::acp::lifecycle::Residency::Cold);
        assert_eq!(
            parked.next_runtime_epoch,
            Some(RuntimeEpoch::new(2).unwrap())
        );
        for owner in owners.iter().skip(1) {
            let page = setup.page(owner);
            let session = page
                .sessions
                .as_slice()
                .iter()
                .find(|session| session.id == owner.session_id)
                .unwrap();
            assert_eq!(session.residency, crate::acp::lifecycle::Residency::Hot);
            assert_eq!(session.runtime_epoch, Some(owner.runtime_epoch));
        }
        setup.send_and_wait_completed(&overflow).await;
        for owner in owners.iter().skip(1).chain(std::iter::once(&overflow)) {
            setup.close(owner);
        }
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}
