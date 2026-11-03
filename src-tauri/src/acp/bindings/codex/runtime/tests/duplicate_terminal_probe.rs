use super::*;
use crate::acp::bindings::codex::{
    hard_failure_tests::with_captured_diagnostics, incoming::UNPROJECTED_DIAGNOSTIC,
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

/// The trial read of a frame the real read reports must report nothing: only
/// the real read, after the handoff, records it.
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

    let (_, diagnostics) =
        with_captured_diagnostics(|| runtime.ingest(&unprojected(), &Mapper).expect("real read"));
    assert_eq!(diagnostics.matches(UNPROJECTED_DIAGNOSTIC).count(), 1);
}
