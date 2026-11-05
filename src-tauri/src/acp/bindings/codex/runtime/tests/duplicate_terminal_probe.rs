use super::*;
use crate::acp::bindings::codex::{
    hard_failure_tests::with_captured_diagnostics,
    incoming::{reports_diagnostics, trial_read},
};

fn finished_turn() -> CodexRuntime {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    runtime
        .ingest(&interrupted(), &Mapper)
        .expect("turn terminal");
    runtime
}

fn interrupted() -> Frame {
    frame(json!({"method":"turn/completed","params":{
        "threadId":"thread-1","turn":{"id":"turn-1","status":"interrupted"}
    }}))
}

#[test]
fn the_probe_admits_repeats_of_the_finished_terminal() {
    let runtime = finished_turn();
    assert!(runtime.frames_repeat_terminal(&[interrupted(), interrupted()], &Mapper));
}

/// The trial read of a frame the real read reports must report nothing, and
/// must leave the thread reporting again for the real read after the handoff.
/// The real read is checked through the reporting gate rather than a second
/// log capture, because the shared capture can miss events under load (#137).
#[test]
fn a_rejected_probe_records_and_logs_nothing() {
    let mut runtime = finished_turn();
    let unprojected = || {
        frame(json!({"method":"turn/diff/updated","params":{
            "threadId":"thread-1"
        }}))
    };
    let (admitted, diagnostics) = with_captured_diagnostics(|| {
        runtime.frames_repeat_terminal(&[interrupted(), unprojected()], &Mapper)
    });
    assert!(!admitted);
    assert_eq!(diagnostics, "");
    assert!(
        reports_diagnostics(),
        "the probe left the trial gate closed"
    );

    assert!(trial_read(|| !reports_diagnostics()));
    assert!(reports_diagnostics(), "a trial read left the gate closed");
    runtime.ingest(&unprojected(), &Mapper).expect("real read");
}

#[test]
fn a_panicking_trial_read_reopens_the_gate() {
    let caught = std::panic::catch_unwind(|| trial_read(|| panic!("trial read failed")));
    assert!(caught.is_err());
    assert!(reports_diagnostics());
}
