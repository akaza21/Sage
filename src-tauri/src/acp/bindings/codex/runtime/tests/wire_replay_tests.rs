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
        // The fixture binding admits the pinned build, which must answer
        // with its own version.
        for record in &mut records {
            if let Some(home) = record.frame.pointer_mut("/result/codexHome") {
                *home = json!(MANAGED_CODEX_HOME);
            }
            // It also echoes the client version, which is this build's.
            if let Some(agent) = record.frame.pointer_mut("/result/userAgent")
                && let Some((product, rest)) = agent.as_str().and_then(|text| text.split_once('/'))
                && let Some((_, details)) = rest.split_once(' ')
                && let Some((platform, _)) = details.rsplit_once(" (")
            {
                *agent = json!(format!(
                    "{product}/{} {platform} (sage; {})",
                    sage_acp_protocol::codex_protocol::PINNED_CODEX_VERSION,
                    env!("CARGO_PKG_VERSION")
                ));
            }
        }
        let mut runtime = fixture_runtime();
        let replay = Replay::run(
            &mut runtime,
            "session-codex-runtime",
            &records,
            &FixtureMapper,
        );
        wire_replay::assert_summary("codex", scenario, &replay.summary(&runtime));
    }
}
