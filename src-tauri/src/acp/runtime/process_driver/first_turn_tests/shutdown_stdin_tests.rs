use super::*;
use crate::acp::process::TreeExit;

/// A harness opened but never prompted still reads its stdin. A shutdown Sage
/// asks for must end that input, or the harness can only sit out its exit
/// grace and be killed.
#[test]
fn a_shutdown_of_an_unprompted_runtime_ends_the_harness_input() {
    sage_test_support::test_wait::guarded("unprompted runtime shutdown", async {
        let FirstTurnFixture {
            project_id,
            chat_id,
            started,
            epoch,
            activation_journal,
            capabilities,
            external_session,
            owner,
            ..
        } = prepare_first_turn_fixture().await;
        let scratch =
            std::env::temp_dir().join(format!("sage-shutdown-stdin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&scratch).unwrap();
        let marker = scratch.join("saved");
        #[cfg(windows)]
        let tree = crate::acp::process::launch_script_for_test(
            91,
            &format!(
                "%SystemRoot%\\System32\\findstr.exe x >nul & type nul > {}",
                marker.display()
            ),
        );
        #[cfg(not(windows))]
        let tree = crate::acp::process::launch_shell_for_test(
            91,
            &format!("cat >/dev/null; : > '{}'", marker.display()),
        );
        let (stdin, _stdout, harness) = tree.into_parts();
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                SequencedAdapter {
                    binding_digest: [0x11; 32],
                    runtime_epoch: epoch,
                    session_id: started.session_id.clone(),
                    external_session_id: external_session.clone(),
                    pending_message: None,
                    prompt_ids: Arc::new(Mutex::new(Vec::new())),
                    cancel_count: Arc::new(AtomicUsize::new(0)),
                    terminal_states: Arc::new(Mutex::new(VecDeque::new())),
                    next_request: 0,
                    next_external_turn: 0,
                },
                activation_journal,
                Mapper,
                ActivatedSession {
                    binding_digest: [0x11; 32],
                    project_id,
                    chat_id,
                    session_id: started.session_id.clone(),
                    external_session_id: external_session,
                    runtime_epoch: epoch,
                    capabilities,
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
            transport: stdin,
            control: NoChildProcessControl {
                zero_checks: Arc::new(AtomicUsize::new(0)),
                harness_error: None,
            },
            owner,
            clock: Arc::new(SystemDriverClock),
        };
        let mut state = FirstTurnWorkerState::Idle(Box::new(completed));
        assert!(state.close_stdin_for_shutdown());
        let begun = std::time::Instant::now();
        harness
            .request_graceful_termination(
                TreeExit::ParentShutdown,
                crate::acp::process::graceful_exit::EXIT_GRACE,
            )
            .unwrap();
        harness.wait_zero().unwrap();
        assert!(marker.exists(), "the harness saw its input end and exited");
        assert!(
            begun.elapsed() < std::time::Duration::from_secs(5),
            "it exited on its own, not at the end of the grace"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    });
}
