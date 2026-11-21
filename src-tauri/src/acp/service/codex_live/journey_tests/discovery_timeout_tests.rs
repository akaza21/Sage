use super::*;
use crate::acp::{
    bindings::codex::CodexDiscoveryDeadline,
    ipc::{AcpIpcError, AcpUnavailableReason, GetCodexDiscoveryRequest},
    runtime::HostSteerIntent,
};
use std::sync::mpsc;

#[test]
fn a_timed_out_account_refresh_leaves_a_streaming_chat_running_until_its_turn_completes() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    tauri::async_runtime::block_on(async {
        // This fixture streams an answer, then holds the turn open until a
        // steer arrives. No sleeps or publisher batching determine the race.
        let setup = FixtureSetup::in_memory(
            CodexBindingRegistration::admitted_for_observed_steer_journey_test(&[
                "codex-steered-message",
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
        let turn = setup.wait_for_running_turn(&owner);
        setup.wait_until(|| {
            setup
                .page(&owner)
                .turns
                .as_slice()
                .iter()
                .any(|turn| !turn.items.as_slice().is_empty())
        });

        let manager = setup.service.codex_discovery_manager();
        let deadline = CodexDiscoveryDeadline::for_test(
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        let prepared = manager
            .prepare(
                GetCodexDiscoveryRequest {
                    refresh: true,
                    project_id: setup.project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
                },
                setup.root(),
                deadline,
            )
            .unwrap();
        let cancel = manager.cancellation(&prepared);
        let (worker_tx, worker_rx) = mpsc::sync_channel(1);
        let task = tauri::async_runtime::spawn_blocking(move || {
            // Hold this admitted refresh past its cleanup deadline.
            let _ = worker_rx.recv_timeout(sage_test_support::test_wait::HANG_GUARD);
            manager.discover(prepared)
        });
        let result =
            crate::commands::acp::codex_discovery::await_codex_discovery(task, deadline, cancel)
                .await;
        worker_tx.send(()).unwrap();
        assert!(matches!(
            result,
            Err(AcpIpcError::Unavailable {
                reason: AcpUnavailableReason::CapabilityUnavailable,
                ..
            })
        ));
        assert_eq!(setup.wait_for_running_turn(&owner), turn);
        let steered = setup.service.dispatch_session_steer(
            &BindingId::new("bindingId", "codex-app-server").unwrap(),
            HostSteerIntent {
                owner: owner.clone(),
                turn_id: turn.clone(),
                prompt: Prompt::new("prompt", "Synthetic steer: use the other example.").unwrap(),
                client_message_id: sage_acp_protocol::contract::RequiredText::new(
                    "clientMessageId",
                    "after-refresh-timeout",
                )
                .unwrap(),
            },
        );
        assert!(
            steered.is_ok(),
            "the original live chat must still accept input"
        );
        setup.wait_for_completed_after(&owner, &[]);
        assert_completed_turns(&setup, &owner, &[turn]);
        setup.close(&owner);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}
