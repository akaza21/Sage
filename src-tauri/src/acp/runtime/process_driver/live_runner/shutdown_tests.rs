use std::time::{Duration, Instant};

use sage_acp_protocol::lifecycle::TurnTerminalReason;

use super::*;
use crate::acp::process::{launch_exiting_for_test, launch_fixture_for_test};

const WAIT: Duration = Duration::from_secs(10);

/// A root that has already exited with `code` before Sage asks it to stop.
fn exited_root(runtime_epoch: u64, code: u8) -> TreeControl {
    let (_, _, control) = launch_exiting_for_test(runtime_epoch, code).into_parts();
    let deadline = Instant::now() + WAIT;
    while control.native_exit_code_if_exited().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the root never exited");
        std::thread::sleep(Duration::from_millis(10));
    }
    control
}

#[test]
fn a_stream_failure_after_the_harness_exited_keeps_its_exit_code() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    for (runtime_epoch, runner_exit) in [
        (1, TreeExit::ProtocolFailure),
        (2, TreeExit::Exited { code: None }),
    ] {
        let control = exited_root(runtime_epoch, 7);
        control.request_termination(runner_exit).unwrap();
        let (root_exit, _token) = control.wait_zero_before(Instant::now() + WAIT).unwrap();
        assert_eq!(root_exit, runner_exit, "the wait reports Sage's request");

        let outcome = lost_runtime_outcome(runner_exit, None, root_exit, &control);

        assert_eq!(outcome.reason, TurnTerminalReason::HarnessExited);
        assert_eq!(
            outcome.message.as_deref(),
            Some("harness process exited with code 7")
        );
    }
}

#[test]
fn a_harness_sage_terminated_keeps_the_runner_cause() {
    let _serial = sage_test_support::test_wait::serial_process_guard();
    let (_, _, control) = launch_fixture_for_test(3).into_parts();
    control
        .request_termination(TreeExit::ProtocolFailure)
        .unwrap();
    let (root_exit, _token) = control.wait_zero_before(Instant::now() + WAIT).unwrap();
    assert_eq!(control.exit_before_termination().unwrap(), None);

    let outcome = lost_runtime_outcome(TreeExit::ProtocolFailure, None, root_exit, &control);

    assert_eq!(outcome.reason, TurnTerminalReason::ProtocolError);
    assert_eq!(
        outcome.message.as_deref(),
        Some("harness process stream failed before a clean exit")
    );
}
