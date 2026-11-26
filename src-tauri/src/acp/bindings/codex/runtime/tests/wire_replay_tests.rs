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
        let mut records = wire_replay::load("codex", scenario);
        // The capture ran Codex on its own home; Sage launches it on a
        // managed one and admits only that.
        for record in &mut records {
            if let Some(home) = record.frame.pointer_mut("/result/codexHome") {
                *home = json!(MANAGED_CODEX_HOME);
            }
        }
        let mut runtime = fixture_runtime();
        let replay = Replay::run(&mut runtime, &records, &FixtureMapper);
        wire_replay::assert_summary("codex", scenario, &replay.summary(&runtime));
    }
}
