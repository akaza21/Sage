//! The native replay catalogue: each journey case's id and scripted steps,
//! and the facts a binding reads from a case (the harness it is for, whether
//! it lists or offers models, whether it spans a restart). Data and pure
//! lookups only; the fixture interpreter that plays a case lives in
//! `acp::fixture`, above the bindings.

#[cfg(any(test, all(windows, debug_assertions)))]
use std::{collections::HashMap, sync::OnceLock};

#[cfg(any(test, all(windows, debug_assertions)))]
use serde_json::Value;

mod generated_journey_cases;
pub(crate) use generated_journey_cases::JOURNEY_CASES;

/// A Codex replay case that answers `model/list`, so Sage reads the model
/// catalogue after the thread opens. Taken from the case's own steps: a case
/// that waits for the request must get it, and one that does not would read
/// it as out of order.
#[cfg(any(test, all(windows, debug_assertions)))]
pub(crate) fn native_case_lists_models(case: &str) -> bool {
    case.starts_with("codex-") && native_case_awaits(case, &["codexModelList"])
}

/// A replay case that runs twice on its profile, so it needs a second launch:
/// one that opens a new session on its first launch and resumes the same one
/// after an app restart, or a Claude Code case that names its relaunch (the
/// fork chat a fork opens, say).
#[cfg(any(test, all(windows, debug_assertions)))]
pub(crate) fn native_case_spans_restart(case: &str) -> bool {
    native_case_awaits(case, &["acpOpen", "codexStartOrResume"])
        || native_case_any_await(case, |step| step["relaunch"] == true)
}

#[cfg(any(test, all(windows, debug_assertions)))]
fn native_case_awaits(case: &str, kinds: &[&str]) -> bool {
    native_case_any_await(case, |step| {
        step["kind"]
            .as_str()
            .is_some_and(|kind| kinds.contains(&kind))
    })
}

#[cfg(any(test, all(windows, debug_assertions)))]
fn native_case_any_await(case: &str, matches: impl Fn(&serde_json::Value) -> bool) -> bool {
    JOURNEY_CASES
        .iter()
        .filter(|entry| entry.id == case)
        .filter_map(|entry| serde_json::from_str::<Vec<serde_json::Value>>(entry.steps_json).ok())
        .any(|steps| {
            steps
                .iter()
                .any(|step| step["op"] == "awaitClient" && matches(step))
        })
}

#[cfg(all(windows, debug_assertions))]
pub(crate) fn native_case_matches(case: &str, protocol: &str) -> bool {
    JOURNEY_CASES
        .iter()
        .any(|entry| entry.id == case && entry.id.starts_with(&format!("{protocol}-")))
}

/// A Pi replay case that drives tool calls through the bundled Sage bridge
/// extension, so its child stands in for a Pi with that extension installed.
#[cfg(all(windows, debug_assertions))]
pub(crate) fn native_case_uses_pi_bridge(case: &str) -> bool {
    case.starts_with("pi-") && native_case_awaits(case, &["piBridgeTool"])
}

/// A Codex case whose thread runs the model it is opened or resumed with, so
/// a model picked in the composer is one the session really uses. Only such a
/// case offers a model choice.
#[cfg(any(test, all(windows, debug_assertions)))]
pub(crate) fn native_case_offers_models(case: &str) -> bool {
    case.starts_with("codex-")
        && steps(case).is_some_and(|steps| {
            steps.iter().any(|step| {
                step["op"] == "emitKnownFrame" && step["shape"] == "codexThreadConfigResult"
            })
        })
        && reply_shape(case, "codexModelList").is_some()
}

/// Each journey case's steps, parsed once.
#[cfg(any(test, all(windows, debug_assertions)))]
fn steps(case: &str) -> Option<&'static [Value]> {
    static STEPS: OnceLock<HashMap<&'static str, Vec<Value>>> = OnceLock::new();
    STEPS
        .get_or_init(|| {
            JOURNEY_CASES
                .iter()
                .filter_map(|entry| Some((entry.id, serde_json::from_str(entry.steps_json).ok()?)))
                .collect()
        })
        .get(case)
        .map(Vec::as_slice)
}

/// The frame the case emits right after it waits for `kind`.
#[cfg(any(test, all(windows, debug_assertions)))]
pub(crate) fn reply_shape(case: &str, kind: &str) -> Option<String> {
    let steps = steps(case)?;
    let wait = steps
        .iter()
        .position(|step| step["op"] == "awaitClient" && step["kind"] == kind)?;
    let reply = steps.get(wait + 1)?;
    (reply["op"] == "emitKnownFrame")
        .then(|| reply["shape"].as_str().map(str::to_owned))
        .flatten()
}
