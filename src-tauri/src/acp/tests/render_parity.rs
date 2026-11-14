//! Cross-harness render parity (docs/harness-render-contract.md).

use serde_json::Value;

use crate::acp::{
    contract::SageRootId,
    intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason},
    lifecycle::{ItemLifecycle, ToolState, TurnState},
};

mod acp;
mod codex;
mod pi;
mod store;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../src/features/chat/acp/harnessRenderParity.fixture.json"
));

pub(super) const ROOT: &str = r"C:\repo";
pub(super) const FILE: &str = r"C:\repo\src\lib.rs";
pub(super) const OLD_TEXT: &str = "pub fn answer() -> u32 {\n    41\n}\n";
pub(super) const NEW_TEXT: &str = "pub fn answer() -> u32 {\n    42\n}\n";
pub(super) const STEER: &str = "Use main.rs instead.";
pub(super) const STEER_CLIENT_ID: &str = "steer-1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Scenario {
    ThinkingThenLinkedText,
    ToolCompletes,
    ToolFailsMidTurn,
    CommandWithOutput,
    FileEditWithDiff,
    PlanUpdate,
    CancelledWithRunningTool,
    SteeredMessage,
}

const SCENARIOS: [(Scenario, &str); 8] = [
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
    (Scenario::SteeredMessage, "steeredMessage"),
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
    /// The user pressed stop: Sage records its durable cancel intent before
    /// it writes the binding's native cancel.
    UserCancel,
}

/// Permission cards need durable request facts this suite does not drive.
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
        AdapterEvent::SteerResponse { accepted: true, .. } => steered_message(),
        _ => return None,
    })
}

/// What Sage's shared layer journals when a harness accepts the steer
/// (`runtime/adapter_pump/steer.rs`). No binding emits the steered message.
pub(super) fn steered_message() -> Fact {
    let (id, payload) = crate::acp::runtime::steered_prompt_item(
        &crate::acp::intent::Prompt::new("prompt", STEER).unwrap(),
        &crate::acp::contract::RequiredText::new("client", STEER_CLIENT_ID).unwrap(),
    )
    .unwrap();
    Fact::Item {
        key: id.as_str().to_owned(),
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        payload,
    }
}

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

/// Store-assigned identities never take part in parity. `cwd` is optional,
/// display-free metadata the store admits only as a mapped resource.
fn strip_volatile(mut value: Value) -> Value {
    match &mut value {
        Value::Object(map) => {
            for key in ["snapshotRevision", "contentDigest", "activityId", "cwd"] {
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
            let actual = match store::project(facts) {
                Ok(actual) => actual,
                Err(error) => {
                    failures.push(format!(
                        "{name}/{harness}: the store refuses the binding's sequence: {error:?}"
                    ));
                    continue;
                }
            };
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

/// The store, not this suite, decides which sequences commit: a binding that
/// emits any of these fails the parity suite.
#[test]
fn the_store_refuses_sequences_no_binding_may_emit() {
    use crate::acp::{contract::SafeText, store::StoreError};
    let text = |lifecycle| Fact::Item {
        key: "message-1".into(),
        lifecycle,
        tool_state: None,
        payload: NormalizedPayload::Text {
            text: SafeText::new("text", "Done.").unwrap(),
        },
    };
    let running_tool = || {
        acp::generic(Scenario::CancelledWithRunningTool)
            .unwrap()
            .into_iter()
            .find(|fact| matches!(fact, Fact::Item { .. }))
            .unwrap()
    };
    let settled_tool = |lifecycle, state| {
        let Fact::Item { key, payload, .. } = running_tool() else {
            unreachable!()
        };
        Fact::Item {
            key,
            lifecycle,
            tool_state: Some(state),
            payload,
        }
    };
    let refused = |facts: Vec<Fact>| store::project(facts).unwrap_err();
    assert!(matches!(
        refused(vec![
            text(ItemLifecycle::Completed),
            text(ItemLifecycle::Streaming)
        ]),
        StoreError::TerminalConflict { resource: "item" }
    ));
    assert!(matches!(
        refused(vec![
            text(ItemLifecycle::Streaming),
            text(ItemLifecycle::Started)
        ]),
        StoreError::InvalidTransition { resource: "item" }
    ));
    assert!(matches!(
        refused(vec![
            settled_tool(ItemLifecycle::Streaming, ToolState::InProgress),
            settled_tool(ItemLifecycle::Streaming, ToolState::Pending),
        ]),
        StoreError::InvalidTransition { resource: "tool" }
    ));
    assert!(matches!(
        refused(vec![settled_tool(
            ItemLifecycle::Completed,
            ToolState::InProgress
        )]),
        StoreError::InvalidTransition {
            resource: "itemShape"
        }
    ));
    assert!(matches!(
        refused(vec![
            Fact::Turn(TurnState::Completed),
            text(ItemLifecycle::Completed)
        ]),
        StoreError::Conflict {
            resource: "itemOwner"
        }
    ));
    assert!(matches!(
        refused(vec![
            Fact::Turn(TurnState::Completed),
            Fact::Turn(TurnState::Failed)
        ]),
        StoreError::TerminalConflict { resource: "turn" }
    ));
    assert!(
        store::project(vec![
            text(ItemLifecycle::Completed),
            Fact::Turn(TurnState::Completed)
        ])
        .is_ok()
    );
}

#[test]
fn a_repeated_steer_accept_keeps_one_user_bubble() {
    let projected = store::project(vec![
        steered_message(),
        steered_message(),
        Fact::Turn(TurnState::Completed),
    ])
    .unwrap();
    let items = projected["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "userPrompt");
    assert_eq!(items[0]["payload"]["text"], STEER);
}
