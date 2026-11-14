use std::time::{Duration, Instant};

use serde_json::json;

use super::super::FixtureError;
use super::super::interpreter::{StepOwner, classify};
use super::{PAUSE_STEP_MAXIMUM_MS, ProcessState, execute};
use crate::acp_protocol::native_cases::JOURNEY_CASES;
use sage_core::limits::LIMITS;

fn run(step: serde_json::Value, state: &mut ProcessState) -> Result<(), FixtureError> {
    let mut exit_code = 0;
    execute(&step, "pause-test", 0, &mut exit_code, state)
}

#[test]
fn pause_is_a_child_process_step() {
    assert!(classify("pauseMs") == Ok(StepOwner::Process));
}

#[test]
fn pause_sleeps_for_its_bounded_duration_and_counts_toward_the_case() {
    let mut state = ProcessState::new();
    let started = Instant::now();
    run(json!({ "op": "pauseMs", "ms": 20 }), &mut state).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(20));
    assert_eq!(state.paused_ms, 20);
}

#[test]
fn pause_rejects_missing_zero_and_oversized_steps() {
    let mut state = ProcessState::new();
    for step in [
        json!({ "op": "pauseMs" }),
        json!({ "op": "pauseMs", "ms": 0 }),
        json!({ "op": "pauseMs", "ms": -5 }),
        json!({ "op": "pauseMs", "ms": PAUSE_STEP_MAXIMUM_MS + 1 }),
    ] {
        assert_eq!(run(step, &mut state), Err(FixtureError::InvalidArguments));
    }
    assert_eq!(state.paused_ms, 0);
}

#[test]
fn pauses_stop_at_the_case_delay_maximum() {
    let mut state = ProcessState::new();
    state.paused_ms = LIMITS.deadlines_ms.fixture_delay_maximum;
    let started = Instant::now();
    assert_eq!(
        run(json!({ "op": "pauseMs", "ms": 1 }), &mut state),
        Err(FixtureError::InstructionLimit)
    );
    assert!(started.elapsed() < Duration::from_millis(PAUSE_STEP_MAXIMUM_MS));
}

#[test]
fn stream_fade_replays_pace_word_level_chunks_within_the_case_bounds() {
    for harness in ["codex", "claude", "pi"] {
        let id = format!("{harness}-motion-stream-fade");
        let case = JOURNEY_CASES
            .iter()
            .find(|case| case.id == id)
            .unwrap_or_else(|| panic!("missing {id}"));
        let steps: Vec<serde_json::Value> = serde_json::from_str(case.steps_json).unwrap();
        assert!(steps.len() as u64 <= LIMITS.fixture.steps_per_case, "{id}");
        let pauses: Vec<u64> = steps
            .iter()
            .filter(|step| step["op"] == "pauseMs")
            .map(|step| step["ms"].as_u64().unwrap())
            .collect();
        assert!(pauses.len() >= 16, "{id} paces its word trail");
        assert!(
            pauses
                .iter()
                .all(|ms| (1..=PAUSE_STEP_MAXIMUM_MS).contains(ms))
        );
        assert!(pauses.iter().sum::<u64>() <= LIMITS.deadlines_ms.fixture_delay_maximum);
        for (index, step) in steps.iter().enumerate() {
            if step["op"] == "pauseMs" {
                assert_eq!(steps[index - 1]["op"], "emitKnownFrame", "{id}");
            }
        }
    }
}

#[test]
fn mcp_progress_replay_holds_each_status_line_long_enough_to_read() {
    let case = JOURNEY_CASES
        .iter()
        .find(|case| case.id == "codex-mcp-progress")
        .expect("the codex-mcp-progress journey exists");
    let steps: Vec<serde_json::Value> = serde_json::from_str(case.steps_json).unwrap();
    let mut held = Vec::new();
    let mut since_visible = None;
    for step in &steps {
        let method = step["method"].as_str().unwrap_or_default();
        if step["op"] == "pauseMs" {
            since_visible = since_visible.map(|ms: u64| ms + step["ms"].as_u64().unwrap());
        } else if method.starts_with("item/") {
            held.extend(since_visible);
            since_visible = Some(0);
        }
    }
    assert_eq!(held.len(), 3);
    assert!(held.iter().all(|ms| *ms >= 2_000), "{held:?}");
    assert!(held.iter().sum::<u64>() <= LIMITS.deadlines_ms.fixture_delay_maximum);
}
