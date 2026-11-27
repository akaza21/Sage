//! Real Claude Code stream-json transcripts (`tests/fixtures/harness-wire/
//! claude`) replayed through the SDK binding. Recapture with
//! `scripts/harness-wire/capture.mjs` when Claude Code changes.

use super::*;
use crate::acp::bindings::wire_replay::{self, FixtureMapper, ROOT, Replay};

fn replayed(scenario: &str) -> Value {
    let mut runtime = runtime_at(ROOT);
    let records = wire_replay::load("claude", scenario);
    let replay = Replay::run(&mut runtime, "session-claude-sdk", &records, &FixtureMapper);
    replay.summary(&runtime)
}

#[test]
fn every_recorded_claude_transcript_projects_as_committed() {
    let scenarios = wire_replay::scenarios("claude");
    assert!(
        scenarios.iter().any(|name| name == "commands"),
        "{scenarios:?}"
    );
    for scenario in &scenarios {
        wire_replay::assert_summary("claude", scenario, &replayed(scenario));
    }
}
