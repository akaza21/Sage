//! Real Pi RPC transcripts (`tests/fixtures/harness-wire/pi`) replayed through
//! the Pi binding. Recapture with `scripts/harness-wire/capture.mjs` when Pi
//! changes.

use super::*;
use crate::acp::bindings::wire_replay::{self, FixtureMapper, Replay};

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
        wire_replay::assert_summary("pi", scenario, &replay.summary(&runtime));
    }
}
