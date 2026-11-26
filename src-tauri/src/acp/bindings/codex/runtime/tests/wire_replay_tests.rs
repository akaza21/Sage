//! Real Codex app-server transcripts (`tests/fixtures/harness-wire/codex`)
//! replayed through the Codex binding. Recapture with
//! `scripts/harness-wire/capture.mjs` when Codex changes.

use super::*;
use crate::acp::bindings::wire_replay::{self, FixtureMapper, Replay};

#[test]
fn every_recorded_codex_transcript_projects_as_committed() {
    let scenarios = wire_replay::scenarios("codex");
    assert!(
        scenarios.iter().any(|name| name == "commands"),
        "{scenarios:?}"
    );
    for scenario in &scenarios {
        let mut runtime = fixture_runtime();
        let records = wire_replay::load("codex", scenario);
        let replay = Replay::run(&mut runtime, &records, &FixtureMapper);
        wire_replay::assert_summary("codex", scenario, &replay.summary(&runtime));
    }
}
