use super::*;
use crate::acp::bindings::adapter::HarnessAdapter;
use sage_acp_protocol::lifecycle::TurnState;

fn reply(runtime: &mut PiRuntime, message: Value) {
    runtime
        .ingest(
            &frame(json!({"type":"message_start","message":{"role":"assistant","content":[]}})),
            &Mapper,
        )
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"type":"message_end","message":message})),
            &Mapper,
        )
        .unwrap();
}

fn settled(runtime: &mut PiRuntime) -> AdapterBatch {
    runtime
        .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
        .unwrap()
}

#[test]
fn a_dropped_end_of_run_event_still_ends_the_turn_from_the_streamed_reply() {
    let mut runtime = fixture_runtime(31);
    ready(&mut runtime);
    running(&mut runtime);
    assert!(runtime.skips_oversize_frames());
    reply(
        &mut runtime,
        json!({"role":"assistant","content":[],"stopReason":"stop","usage":{"input":7,"output":3,"cacheRead":2}}),
    );
    runtime.oversize_frame_skipped();
    let batch = settled(&mut runtime);
    assert!(
        matches!(
            batch.events(),
            [
                AdapterEvent::Usage {
                    payload: NormalizedPayload::Usage {
                        input_tokens: 9,
                        output_tokens: 3,
                        ..
                    },
                    ..
                },
                AdapterEvent::Turn {
                    state: TurnState::Completed,
                    ..
                }
            ]
        ),
        "{:?}",
        batch.events()
    );
}

#[test]
fn a_dropped_end_of_run_event_keeps_a_failed_reply_failed() {
    let mut runtime = fixture_runtime(32);
    ready(&mut runtime);
    running(&mut runtime);
    reply(
        &mut runtime,
        json!({"role":"assistant","content":[],"stopReason":"error","errorMessage":"rate limited"}),
    );
    runtime.oversize_frame_skipped();
    let batch = settled(&mut runtime);
    assert!(
        batch.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Failed,
                ..
            }
        )),
        "{:?}",
        batch.events()
    );
}

#[test]
fn settling_without_an_end_of_run_event_is_still_out_of_order_when_nothing_was_dropped() {
    let mut runtime = fixture_runtime(33);
    ready(&mut runtime);
    running(&mut runtime);
    reply(
        &mut runtime,
        json!({"role":"assistant","content":[],"stopReason":"stop"}),
    );
    assert!(
        runtime
            .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
            .is_err()
    );
}

#[test]
fn an_outstanding_read_keeps_oversized_frames_fatal() {
    let mut runtime = fixture_runtime(34);
    let state = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    deliver(&mut runtime, state);
    assert!(!runtime.skips_oversize_frames());
}

#[test]
fn oversized_frames_stay_fatal_inside_a_reply_and_between_turns() {
    let mut runtime = fixture_runtime(35);
    ready(&mut runtime);
    assert!(!runtime.skips_oversize_frames());
    running(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"type":"message_start","message":{"role":"assistant","content":[]}})),
            &Mapper,
        )
        .unwrap();
    assert!(!runtime.skips_oversize_frames());
    runtime
        .ingest(
            &frame(json!({"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"stop"}})),
            &Mapper,
        )
        .unwrap();
    assert!(runtime.skips_oversize_frames());
}

#[test]
fn a_retry_after_a_dropped_end_of_run_event_settles_from_the_retried_reply() {
    let mut runtime = fixture_runtime(36);
    ready(&mut runtime);
    running(&mut runtime);
    reply(
        &mut runtime,
        json!({"role":"assistant","content":[],"stopReason":"error","errorMessage":"overloaded"}),
    );
    runtime.oversize_frame_skipped();
    runtime
        .ingest(&frame(json!({"type":"agent_start"})), &Mapper)
        .expect("a new run after a dropped end of run");
    reply(
        &mut runtime,
        json!({"role":"assistant","content":[],"stopReason":"stop"}),
    );
    runtime.oversize_frame_skipped();
    let batch = settled(&mut runtime);
    assert!(
        batch.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: TurnState::Completed,
                ..
            }
        )),
        "{:?}",
        batch.events()
    );
}
