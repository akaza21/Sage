use super::*;
use crate::acp::{bindings::adapter::HarnessAdapter, lifecycle::TurnState};

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
    // `agent_end` repeated the whole run and outgrew a frame.
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
    // Initialization reads Pi's state and models; their answers may be large.
    let state = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    deliver(&mut runtime, state);
    assert!(!runtime.skips_oversize_frames());
}
