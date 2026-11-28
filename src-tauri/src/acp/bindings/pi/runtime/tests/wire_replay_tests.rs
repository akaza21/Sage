//! Real Pi RPC transcripts (`tests/fixtures/harness-wire/pi`) replayed through
//! the Pi binding. Recapture with `scripts/harness-wire/capture.mjs` when Pi
//! changes.

use super::*;
use crate::acp::bindings::{
    wire_contract,
    wire_replay::{self, FixtureMapper, Replay},
};

#[test]
fn every_recorded_pi_transcript_projects_as_committed() {
    let scenarios = wire_replay::scenarios("pi");
    assert!(
        scenarios.iter().any(|name| name == "commands"),
        "{scenarios:?}"
    );
    for scenario in &scenarios {
        let mut runtime = fixture_runtime(1);
        let records = wire_replay::load("pi", scenario);
        let replay = Replay::run(&mut runtime, "session-pi-1", &records, &FixtureMapper);
        wire_contract::assert_conforms("pi", scenario, &replay.transcript);
        wire_replay::assert_summary("pi", scenario, &replay.summary(&runtime));
    }
}

/// An event kind a newer Pi adds arrives between turns too; the runtime
/// must not fail ingest (and with it the launch) over it.
#[test]
fn an_event_kind_pi_does_not_define_never_fails_ingest_between_turns() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let batch = runtime
        .ingest(&frame(json!({"type":"a_future_event"})), &Mapper)
        .unwrap();
    assert!(
        batch.events().is_empty()
            || batch
                .events()
                .iter()
                .all(|event| matches!(event, AdapterEvent::Absorbed)),
        "{:?}",
        batch.events()
    );
}
