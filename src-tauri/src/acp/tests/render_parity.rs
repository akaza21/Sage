//! Cross-harness render parity (docs/harness-render-contract.md).
//!
//! One scenario set is driven through each harness's real binding, folded
//! into the frontend item projection the store would decode, and compared
//! with the single canonical projection in the shared fixture the chat view
//! test renders. A harness that cannot produce a scenario is listed as not
//! applicable; a known divergence is pinned with the exact fields the binding
//! emits differently today, so fixing it or diverging further fails until the
//! owning thread updates the fixture.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::acp::{
    contract::SageRootId,
    file_diff::{PairAdmission, admit_pair},
    intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason},
    lifecycle::{ItemLifecycle, ToolState, TurnState},
    store::project_item_content,
};

mod acp;
mod codex;
mod pi;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../src/features/chat/acp/harnessRenderParity.fixture.json"
));

pub(super) const ROOT: &str = r"C:\repo";
pub(super) const FILE: &str = r"C:\repo\src\lib.rs";
pub(super) const OLD_TEXT: &str = "pub fn answer() -> u32 {\n    41\n}\n";
pub(super) const NEW_TEXT: &str = "pub fn answer() -> u32 {\n    42\n}\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scenario {
    ThinkingThenLinkedText,
    ToolCompletes,
    ToolFailsMidTurn,
    CommandWithOutput,
    FileEditWithDiff,
    PlanUpdate,
    CancelledWithRunningTool,
}

const SCENARIOS: [(Scenario, &str); 7] = [
    (Scenario::ThinkingThenLinkedText, "thinkingThenLinkedText"),
    (Scenario::ToolCompletes, "toolCompletes"),
    (Scenario::ToolFailsMidTurn, "toolFailsMidTurn"),
    (Scenario::CommandWithOutput, "commandWithOutput"),
    (Scenario::FileEditWithDiff, "fileEditWithDiff"),
    (Scenario::PlanUpdate, "planUpdate"),
    (
        Scenario::CancelledWithRunningTool,
        "cancelledWithRunningTool",
    ),
];

type Driver = fn(Scenario) -> Option<Vec<Fact>>;

/// Claude's current binding and a generic ACP agent share the standard ACP
/// translator; they differ only by profile.
const HARNESSES: [(&str, Driver); 4] = [
    ("claude", acp::claude),
    ("codex", codex::run),
    ("pi", pi::run),
    ("acp", acp::generic),
];

/// What a binding hands the store, stripped of its identity envelope.
#[derive(Debug)]
pub(super) enum Fact {
    Item {
        key: String,
        lifecycle: ItemLifecycle,
        tool_state: Option<ToolState>,
        payload: NormalizedPayload,
    },
    Pair {
        key: String,
        old_text: String,
        new_text: String,
    },
    Turn(TurnState),
}

/// A native runtime's event, stripped of its identity envelope. Permission
/// cards are projected from durable request facts, not from this fold.
pub(super) fn adapter_fact(event: crate::acp::bindings::adapter::AdapterEvent) -> Option<Fact> {
    use crate::acp::bindings::adapter::AdapterEvent;
    Some(match event {
        AdapterEvent::Item {
            id,
            lifecycle,
            tool_state,
            payload,
        } => Fact::Item {
            key: id.external_id.as_str().to_owned(),
            lifecycle,
            tool_state,
            payload,
        },
        AdapterEvent::FilePair {
            id,
            old_text,
            new_text,
            ..
        } => Fact::Pair {
            key: id.external_id.as_str().to_owned(),
            old_text,
            new_text,
        },
        AdapterEvent::Turn { state, .. } => Fact::Turn(state),
        _ => return None,
    })
}

/// Maps paths under one project root, `root-1`.
pub(super) struct Mapper(pub(super) String);

impl Mapper {
    pub(super) fn repo() -> Self {
        Self(ROOT.to_owned())
    }

    fn map(&self, path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        let relative = path
            .strip_prefix(self.0.as_str())
            .and_then(|rest| rest.strip_prefix(['\\', '/']))
            .ok_or(UnavailableResourceReason::Foreign)?;
        SafeResourceRef::new(
            SageRootId::parse("root-1").expect("root"),
            relative.replace('\\', "/"),
        )
        .map_err(|_| UnavailableResourceReason::Unsupported)
    }
}

impl crate::acp::bindings::adapter::ResourceMapper for Mapper {
    fn map_existing(&self, path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        self.map(path)
    }

    fn read_diff_context(&self, _: &SafeResourceRef) -> Option<String> {
        Some(OLD_TEXT.to_owned())
    }
}

impl crate::acp::bindings::standard_acp::translator::ResourceMapper for Mapper {
    fn map_absolute(&self, path: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        self.map(path)
    }
}

pub(super) fn frame(value: &Value) -> crate::acp::transport::Frame {
    let mut bytes = serde_json::to_vec(value).expect("json");
    bytes.push(b'\n');
    crate::acp::transport::IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .expect("valid frame")
        .expect("complete frame")
}

/// Folds facts the way the store commits them: the latest snapshot of each
/// item replaces the earlier one in first-seen order, a retained pair makes
/// the diff available, and a terminal Turn sweeps still-open items
/// (`store/terminalize.rs`).
fn project(facts: Vec<Fact>) -> Value {
    let mut order = Vec::new();
    let mut items: BTreeMap<String, (ItemLifecycle, Option<ToolState>, NormalizedPayload)> =
        BTreeMap::new();
    let mut turn = TurnState::Running;
    for fact in facts {
        match fact {
            Fact::Item {
                key,
                lifecycle,
                tool_state,
                payload,
            } => {
                if !items.contains_key(&key) {
                    order.push(key.clone());
                }
                items.insert(key, (lifecycle, tool_state, payload));
            }
            Fact::Pair {
                key,
                old_text,
                new_text,
            } => {
                let Some((_, _, payload)) = items.get_mut(&key) else {
                    panic!("pair {key} precedes its item")
                };
                retain_pair(payload, &old_text, &new_text);
            }
            Fact::Turn(state) => turn = state,
        }
    }
    let items = order
        .into_iter()
        .map(|key| {
            let (mut lifecycle, mut tool_state, mut payload) = items.remove(&key).unwrap();
            if turn.is_terminal() && !lifecycle.is_terminal() {
                lifecycle = ItemLifecycle::Cancelled;
                if tool_state.is_some_and(|state| !state.is_terminal()) {
                    tool_state = Some(ToolState::Cancelled);
                }
            }
            if let NormalizedPayload::FileChangeDiff {
                pair_available: true,
                snapshot_revision,
                ..
            } = &mut payload
            {
                *snapshot_revision = 1;
            }
            let content = project_item_content(payload, lifecycle).expect("projectable");
            let mut row = Map::new();
            row.insert("lifecycle".into(), json!(lifecycle));
            row.insert("toolState".into(), json!(tool_state));
            let Value::Object(content) = serde_json::to_value(content).unwrap() else {
                unreachable!()
            };
            row.extend(content);
            strip_volatile(Value::Object(row))
        })
        .collect::<Vec<_>>();
    json!({ "turnState": turn, "items": items })
}

fn retain_pair(payload: &mut NormalizedPayload, old: &str, new: &str) {
    let NormalizedPayload::FileChangeDiff {
        truncated,
        snapshot_revision,
        old_bytes,
        new_bytes,
        content_digest,
        pair_available,
        ..
    } = payload
    else {
        panic!("a pair belongs to a file change")
    };
    let PairAdmission::Admitted {
        old_bytes: old_len,
        new_bytes: new_len,
        digest,
    } = admit_pair(old, new)
    else {
        panic!("scenario pairs fit")
    };
    (*truncated, *snapshot_revision, *pair_available) = (false, 1, true);
    (*old_bytes, *new_bytes, *content_digest) = (old_len as u64, new_len as u64, digest);
}

/// Store-assigned identities never take part in parity.
fn strip_volatile(mut value: Value) -> Value {
    match &mut value {
        Value::Object(map) => {
            for key in ["snapshotRevision", "contentDigest", "activityId"] {
                map.remove(key);
            }
            for child in map.values_mut() {
                *child = strip_volatile(child.take());
            }
        }
        Value::Array(values) => {
            for child in values {
                *child = strip_volatile(child.take());
            }
        }
        _ => {}
    }
    value
}

fn listed<'a>(fixture: &'a Value, list: &str, scenario: &str, harness: &str) -> Option<&'a Value> {
    fixture[list]
        .as_array()
        .expect("fixture list")
        .iter()
        .find(|entry| entry["scenario"] == scenario && entry["harness"] == harness)
}

/// The canonical projection with a gap's known differences in place: each
/// JSON pointer names a field the binding emits differently today.
fn with_differences(canonical: &Value, differs: &Value) -> Value {
    let mut expected = canonical.clone();
    for (pointer, value) in differs.as_object().expect("differs") {
        *expected
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("{pointer} is not in the canonical projection")) =
            value.clone();
    }
    expected
}

#[test]
fn every_harness_projects_each_scenario_to_the_one_canonical_transcript() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture json");
    let mut failures = Vec::new();
    let mut cells = 0;
    for (scenario, name) in SCENARIOS {
        let canonical = &fixture["scenarios"][name]["projection"];
        assert!(canonical.is_object(), "{name} has a canonical projection");
        for (harness, driver) in HARNESSES {
            let not_applicable = listed(&fixture, "notApplicable", name, harness);
            let gap = listed(&fixture, "gaps", name, harness);
            let Some(facts) = driver(scenario) else {
                if not_applicable.is_none() {
                    failures.push(format!(
                        "{name}/{harness}: no wire script and not listed N/A"
                    ));
                }
                continue;
            };
            cells += 1;
            let actual = project(facts);
            let expected = match (not_applicable, gap) {
                (Some(_), _) => {
                    failures.push(format!("{name}/{harness}: listed N/A but has a script"));
                    continue;
                }
                (None, Some(gap)) => {
                    if actual == *canonical {
                        failures.push(format!(
                            "{name}/{harness}: gap fixed; drop it from the fixture ({})",
                            gap["gap"]
                        ));
                        continue;
                    }
                    with_differences(canonical, &gap["differs"])
                }
                (None, None) => canonical.clone(),
            };
            if actual != expected {
                failures.push(format!(
                    "{name}/{harness}: projection diverges\nexpected {}\nactual   {}",
                    expected, actual
                ));
            }
        }
    }
    assert!(cells > 0);
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn parity_fixture_lists_only_known_scenarios_harnesses_and_owners() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("fixture json");
    let scenarios: Vec<_> = SCENARIOS.iter().map(|(_, name)| *name).collect();
    let harnesses: Vec<_> = HARNESSES.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        fixture["scenarios"].as_object().unwrap().len(),
        scenarios.len()
    );
    for list in ["notApplicable", "gaps"] {
        for entry in fixture[list].as_array().unwrap() {
            let scenario = entry["scenario"].as_str().unwrap();
            let harness = entry["harness"].as_str().unwrap();
            assert!(scenarios.contains(&scenario), "{entry}");
            assert!(harnesses.contains(&harness), "{entry}");
            assert!(
                entry["reason"].is_string() || entry["gap"].is_string(),
                "{entry}"
            );
        }
    }
    for gap in fixture["gaps"].as_array().unwrap() {
        assert!(
            matches!(
                gap["owner"].as_str(),
                Some("Claude" | "Codex" | "Pi" | "ACP")
            ),
            "{gap}"
        );
        let differs = gap["differs"].as_object().expect("differs");
        assert!(!differs.is_empty(), "{gap}");
    }
}
