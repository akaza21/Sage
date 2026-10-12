use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value, json};

use super::{
    handshake::{StandardAcpHandshake, StandardAcpHandshakeError},
    profile::{StandardAcpProfile, profile_for},
    translator::{AdapterEvent, PromptTerminal, ResourceMapper, StandardAcpTranslator},
};
use crate::acp::intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason};

const UNASSERTED_EXPECTED_KEYS: &[&str] = &["updateMode"];

struct ForeignResources;

impl ResourceMapper for ForeignResources {
    fn map_absolute(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

struct Scenario {
    harness: String,
    scenario: String,
    directory: PathBuf,
}

impl Scenario {
    fn name(&self) -> String {
        format!("{}/{}", self.harness, self.scenario)
    }

    fn expected(&self) -> Value {
        let raw = fs::read_to_string(self.directory.join("expected.json")).unwrap_or_else(|_| {
            panic!(
                "{} has no expected.json; every captured trace must declare a contract, even one that only records why it is blocked",
                self.name()
            )
        });
        serde_json::from_str(&raw).expect("expected.json is valid JSON")
    }

    fn frames(&self) -> Vec<Value> {
        fs::read_to_string(self.directory.join("frames.jsonl"))
            .expect("frames.jsonl is readable")
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("each captured line is valid JSON"))
            .collect()
    }
}

fn traces_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("harness-traces")
}

fn scenarios() -> Vec<Scenario> {
    let mut found = Vec::new();
    for harness in fs::read_dir(traces_root()).expect("trace root is readable") {
        let harness = harness.expect("trace harness entry");
        if !harness.file_type().expect("harness file type").is_dir() {
            continue;
        }
        for scenario in fs::read_dir(harness.path()).expect("harness directory is readable") {
            let scenario = scenario.expect("trace scenario entry");
            if !scenario.file_type().expect("scenario file type").is_dir() {
                continue;
            }
            found.push(Scenario {
                harness: harness.file_name().to_string_lossy().into_owned(),
                scenario: scenario.file_name().to_string_lossy().into_owned(),
                directory: scenario.path(),
            });
        }
    }
    found.sort_by_key(Scenario::name);
    found
}

fn direction(frame: &Value) -> &str {
    frame.get("direction").and_then(Value::as_str).unwrap_or("")
}

fn wire(frame: &Value) -> &Value {
    frame.get("frame").unwrap_or(&Value::Null)
}

fn request_id_of(frames: &[Value], method: &str) -> Option<Value> {
    frames
        .iter()
        .filter(|frame| direction(frame) == "out")
        .map(wire)
        .find(|frame| frame.get("method").and_then(Value::as_str) == Some(method))
        .and_then(|frame| frame.get("id").cloned())
}

fn response_with_id(frames: &[Value], id: u64) -> Option<&Value> {
    frames
        .iter()
        .filter(|frame| direction(frame) == "in")
        .map(wire)
        .find(|frame| frame.get("id").and_then(Value::as_u64) == Some(id))
}

fn handshake_outcome(profile: &'static StandardAcpProfile, frames: &[Value]) -> &'static str {
    let mut runtime = StandardAcpHandshake::new(profile);
    if runtime.initialize().is_err() {
        return "initializeUnavailable";
    }
    let Some(initialized) = response_with_id(frames, 1) else {
        return "noInitializeResponse";
    };
    if runtime.accept_initialize(initialized).is_err() {
        return "initializeRejected";
    }
    if runtime.new_session(".").is_err() {
        return "sessionUnavailable";
    }
    let Some(opened) = response_with_id(frames, 2) else {
        return "noSessionResponse";
    };
    match runtime.accept_new_session(opened) {
        Ok(_) => "session",
        Err(StandardAcpHandshakeError::SignInRequired) => "signInRequired",
        Err(_) => "sessionRejected",
    }
}

fn session_id_of(frames: &[Value]) -> Option<String> {
    let request = request_id_of(frames, "session/new")?;
    frames
        .iter()
        .filter(|frame| direction(frame) == "in")
        .map(wire)
        .find(|frame| frame.get("id") == Some(&request))
        .and_then(|frame| frame.pointer("/result/sessionId"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn observed_events(profile: &'static StandardAcpProfile, frames: &[Value]) -> Vec<Value> {
    let Some(session_id) = session_id_of(frames) else {
        return Vec::new();
    };
    let prompt_id = request_id_of(frames, "session/prompt");
    let mut translator = StandardAcpTranslator::new(profile, session_id).expect("translator");
    if let Some(prompt_id) = prompt_id.as_ref() {
        translator
            .begin_prompt(&rpc_id_text(prompt_id))
            .expect("prompt opens");
    }
    let mut observed = Vec::new();
    for frame in frames.iter().filter(|frame| direction(frame) == "in") {
        let frame = wire(frame);
        let method = frame.get("method").and_then(Value::as_str);
        let translated = match (frame.get("id").is_some(), method) {
            (true, Some("session/request_permission")) => {
                translator.translate_permission_request_value(frame.clone())
            }
            (true, Some(method)) => {
                observed.push(json!({ "event": "clientRequest", "method": method }));
                continue;
            }
            (false, Some(_)) => {
                translator.translate_notification_value(frame.clone(), &ForeignResources)
            }
            (true, None) if prompt_id.is_some() && frame.get("id") == prompt_id.as_ref() => {
                translator.translate_prompt_result_value(frame.clone())
            }
            _ => continue,
        };
        match translated {
            Ok(events) => observed.extend(events.iter().filter_map(observed_event)),
            Err(error) => observed.push(json!({
                "event": "refused",
                "method": method.unwrap_or("response"),
                "error": format!("{error:?}"),
            })),
        }
    }
    observed
}

fn rpc_id_text(id: &Value) -> String {
    id.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| id.to_string())
}

fn observed_event(event: &AdapterEvent) -> Option<Value> {
    match event {
        AdapterEvent::Item {
            external_key,
            lifecycle,
            tool_state,
            payload,
        } => {
            let observed = match payload {
                NormalizedPayload::Unsupported {
                    external_kind,
                    summary,
                } => json!({
                    "event": "unsupported",
                    "externalKind": external_kind.as_str(),
                    "reason": summary.as_str(),
                }),
                NormalizedPayload::Error {
                    code,
                    message,
                    retryable,
                    ..
                } => json!({
                    "event": "error",
                    "code": code.as_str(),
                    "message": message.as_str(),
                    "retryable": retryable,
                }),
                _ => {
                    let mut item = json!({
                        "event": "item",
                        "externalItemId": external_key.as_str(),
                        "kind": payload.kind().as_str(),
                        "lifecycle": camel(&format!("{lifecycle:?}")),
                    });
                    payload_fields(payload, item.as_object_mut().expect("item object"));
                    if let Some(state) = tool_state {
                        item["toolState"] = json!(camel(&format!("{state:?}")));
                    }
                    item
                }
            };
            Some(observed)
        }
        AdapterEvent::TurnTerminal(terminal) => Some(json!({
            "event": "turn",
            "state": match terminal {
                PromptTerminal::Completed => "completed",
                PromptTerminal::Cancelled => "cancelled",
                PromptTerminal::Failed => "failed",
            },
        })),
        AdapterEvent::TurnRunning => Some(json!({ "event": "turn", "state": "running" })),
        AdapterEvent::Usage(payload) => {
            let NormalizedPayload::Usage {
                input_tokens,
                output_tokens,
                context_window,
            } = payload
            else {
                return None;
            };
            Some(json!({
                "event": "usage",
                "inputTokens": input_tokens,
                "outputTokens": output_tokens,
                "contextWindow": context_window,
            }))
        }
        AdapterEvent::FilePair { external_key, .. } => Some(json!({
            "event": "filePair",
            "externalItemId": external_key.as_str(),
        })),
        AdapterEvent::PermissionDecline { request_id, .. } => Some(json!({
            "event": "permissionDecline",
            "externalRequestId": request_id.as_str(),
        })),
        AdapterEvent::Absorbed | AdapterEvent::IgnoredDuplicate | AdapterEvent::IgnoredLate => None,
    }
}

fn payload_fields(payload: &NormalizedPayload, item: &mut Map<String, Value>) {
    match payload {
        NormalizedPayload::Text { text } => {
            item.insert("text".into(), json!(text.as_str()));
        }
        NormalizedPayload::ThoughtSummary { summary } => {
            item.insert("text".into(), json!(summary.as_str()));
        }
        NormalizedPayload::ToolCall {
            category,
            label,
            input_summary,
            output_summary,
            ..
        } => {
            item.insert("category".into(), json!(camel(&format!("{category:?}"))));
            item.insert("label".into(), json!(label.as_str()));
            if let Some(input) = input_summary {
                item.insert("inputSummary".into(), json!(input.as_str()));
            }
            if let Some(output) = output_summary {
                item.insert("outputSummary".into(), json!(output.as_str()));
            }
        }
        _ => {}
    }
}

fn camel(variant: &str) -> String {
    let mut characters = variant.chars();
    match characters.next() {
        Some(first) => first.to_lowercase().chain(characters).collect(),
        None => String::new(),
    }
}

fn asserted_expectations(expected: &Value) -> Vec<Value> {
    let tool_call_id = expected.get("toolCallId").and_then(Value::as_str);
    let mut asserted = Vec::new();
    for entry in expected
        .get("events")
        .and_then(Value::as_array)
        .expect("expected.json declares events")
    {
        if entry.get("event").and_then(Value::as_str) == Some("sessionReady") {
            continue;
        }
        let mut entry = entry.clone();
        let object = entry.as_object_mut().expect("expected entry is an object");
        for key in UNASSERTED_EXPECTED_KEYS {
            object.remove(*key);
        }
        if let (Some(tool_call_id), Some(Value::String(external))) =
            (tool_call_id, object.get("externalItemId"))
            && external == "$toolCallId"
        {
            object.insert("externalItemId".into(), json!(tool_call_id));
        }
        let repeats = object
            .remove("chunks")
            .and_then(|chunks| chunks.as_u64())
            .unwrap_or(1);
        for _ in 0..repeats {
            asserted.push(entry.clone());
        }
    }
    asserted
}

fn divergences(scenario: &Scenario, observed: &[Value], expected: &[Value]) -> Vec<String> {
    let mut found = Vec::new();
    for (index, expectation) in expected.iter().enumerate() {
        let Some(actual) = observed.get(index) else {
            found.push(format!(
                "{}: the trace stopped after {index} events but the contract expects {expectation}",
                scenario.name()
            ));
            break;
        };
        for (key, value) in expectation.as_object().expect("expectation object") {
            if actual.get(key) != Some(value) {
                found.push(format!(
                    "{}: event {index} disagrees on {key}\n  observed: {actual}\n  expected: {expectation}",
                    scenario.name()
                ));
            }
        }
    }
    if observed.len() > expected.len() {
        found.push(format!(
            "{}: the adapter emitted {} events the contract does not describe, starting at {}",
            scenario.name(),
            observed.len() - expected.len(),
            observed[expected.len()]
        ));
    }
    if !found.is_empty() {
        found.push(format!(
            "{} observed in full:\n{}",
            scenario.name(),
            serde_json::to_string_pretty(&observed).expect("observed events")
        ));
    }
    found
}

#[test]
fn every_captured_trace_projects_exactly_what_its_contract_declares() {
    let mut asserted = 0;
    let mut found = Vec::new();
    for scenario in scenarios() {
        let expected = scenario.expected();
        let Some(profile) = profile_for(&scenario.harness) else {
            assert!(
                pending_blocker(&expected).is_some(),
                "{} has no profile and declares no blocker explaining why",
                scenario.name()
            );
            continue;
        };
        let asserted_events = asserted_expectations(&expected);
        assert!(
            !asserted_events.is_empty() || pending_blocker(&expected).is_some(),
            "{} asserts no events and declares no blocker explaining why",
            scenario.name()
        );
        let observed = observed_events(profile, &scenario.frames());
        found.extend(divergences(&scenario, &observed, &asserted_events));
        asserted += 1;
    }
    assert!(
        found.is_empty(),
        "{} contract divergences across every replayed trace:\n\n{}",
        found.len(),
        found.join("\n\n")
    );
    assert!(asserted >= 3, "the suite replayed only {asserted} traces");
}

fn pending_blocker(expected: &Value) -> Option<&str> {
    expected.get("blocker").and_then(Value::as_str)
}

#[test]
fn every_captured_handshake_reaches_the_outcome_its_contract_declares() {
    for scenario in scenarios() {
        let Some(profile) = profile_for(&scenario.harness) else {
            continue;
        };
        let expected = scenario.expected();
        let declared = expected
            .get("handshake")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{} declares no handshake outcome", scenario.name()));
        assert_eq!(
            declared,
            handshake_outcome(profile, &scenario.frames()),
            "{} opens its session differently than its contract declares",
            scenario.name()
        );
    }
}

#[test]
fn every_trace_directory_is_either_asserted_or_declares_why_it_cannot_be() {
    for scenario in scenarios() {
        let parked: Vec<String> = fs::read_dir(&scenario.directory)
            .expect("scenario directory is readable")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("expected.json") && name != "expected.json")
            .collect();
        assert!(
            parked.is_empty(),
            "{} parks its contract as {parked:?}; a trace may declare a blocker but may never be hidden from the suite",
            scenario.name()
        );
        let expected = scenario.expected();
        assert!(
            expected.get("events").and_then(Value::as_array).is_some(),
            "{} declares no events",
            scenario.name()
        );
    }
}
