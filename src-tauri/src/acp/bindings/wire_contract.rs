//! Checks every frame a binding writes during a corpus replay against the
//! harness's official protocol definitions, at the contract of the recorded
//! harness version (`tests/fixtures/harness-contracts/<harness>`, made
//! by `scripts/harness-wire/contract.mjs` from the harness's own published
//! schema or types). Frame kinds and the field walk are those of
//! `shape.mjs` and `contract.mjs`; a test keeps the kinds in step.

use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value};

use super::wire_replay::corpus;

/// Subtrees whose keys are the model's or the user's data (`shape.mjs`).
const OPAQUE: [&str; 12] = [
    "input",
    "updatedInput",
    "arguments",
    "args",
    "settings",
    "applied",
    "effective",
    "env",
    "properties",
    "structuredContent",
    "partialResult",
    "result_json",
];

fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

/// A protocol-agnostic name for one frame, as `frameKind` in `shape.mjs`
/// names it. `sent` maps the other side's request ids to what they asked.
pub(crate) fn frame_kind(frame: &Value, sent: &HashMap<String, String>) -> String {
    let id = |value: &Value| match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    if let Some(method) = text(&frame["method"]) {
        let request = if frame.get("id").is_some() {
            "request"
        } else {
            "notify"
        };
        let item = text(&frame["params"]["item"]["type"]);
        return [Some(request), Some(method), item]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(":");
    }
    if let Some(answered) = frame.get("id").filter(|_| text(&frame["type"]).is_none()) {
        // JavaScript truthiness, as `frame.error ? ...` reads it.
        let truthy = |value: &Value| match value {
            Value::Null => false,
            Value::Bool(flag) => *flag,
            Value::Number(number) => number.as_f64() != Some(0.0),
            Value::String(text) => !text.is_empty(),
            _ => true,
        };
        let outcome = if frame.get("error").is_some_and(truthy) {
            "error"
        } else {
            "result"
        };
        let name = sent.get(&id(answered)).map_or("?", String::as_str);
        return format!("{outcome}:{name}");
    }
    let mut parts = vec![text(&frame["type"]), text(&frame["subtype"])];
    match text(&frame["type"]) {
        Some("response") => parts.push(
            text(&frame["command"]).or_else(|| sent.get(&id(&frame["id"])).map(String::as_str)),
        ),
        Some("control_request") => parts.push(text(&frame["request"]["subtype"])),
        Some("control_response") => {
            parts.push(text(&frame["response"]["subtype"]));
            parts.push(
                sent.get(&id(&frame["response"]["request_id"]))
                    .map(String::as_str),
            );
        }
        _ => {}
    }
    if frame.get("event").is_some_and(Value::is_object) {
        let event = &frame["event"];
        parts.extend([
            text(&event["type"]),
            text(&event["delta"]["type"]),
            text(&event["content_block"]["type"]),
        ]);
    }
    parts.push(text(&frame["assistantMessageEvent"]["type"]));
    parts.into_iter().flatten().collect::<Vec<_>>().join("/")
}

/// Each frame of a transcript (`true` = written by the client) with its kind.
pub(crate) fn kinded(transcript: &[(bool, Value)]) -> Vec<(bool, String)> {
    let mut sent: [HashMap<String, String>; 2] = Default::default();
    transcript
        .iter()
        .map(|(out, frame)| {
            let kind = frame_kind(frame, &sent[usize::from(!*out)]);
            let id = frame
                .get("request_id")
                .filter(|id| !id.is_null())
                .or_else(|| frame.get("id"));
            // `method ?? request.subtype ?? type`: only an absent or null
            // member falls through, and an empty name records nothing.
            let name = [
                &frame["method"],
                &frame["request"]["subtype"],
                &frame["type"],
            ]
            .into_iter()
            .find(|value| !value.is_null())
            .and_then(text);
            if let (Some(id), Some(name)) = (id, name) {
                let id = id.as_str().map_or_else(|| id.to_string(), str::to_owned);
                sent[usize::from(*out)].insert(id, name.to_owned());
            }
            (*out, kind)
        })
        .collect()
}

/// The newest contract of `harness` at or below `version`, if it has any.
pub(crate) fn contract_for(harness: &str, version: &str) -> Option<Value> {
    use super::release_version::ReleaseVersion;
    // `2.1.332 (Claude Code)`, `codex-cli 0.167.1` and `1.5.0` alike.
    let release = version
        .split_ascii_whitespace()
        .find_map(ReleaseVersion::parse)?;
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/harness-contracts")
        .join(harness);
    let newest = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let stem = path.file_stem()?.to_str()?;
            let found = ReleaseVersion::parse(stem)?;
            (found <= release).then_some((found, path))
        })
        .max_by(|a, b| a.0.cmp(&b.0))?;
    let text = std::fs::read_to_string(&newest.1).expect("contract");
    Some(serde_json::from_str(&text).expect("contract JSON"))
}

fn undefined_fields(
    types: &Map<String, Value>,
    ids: &BTreeSet<&str>,
    value: &Value,
    prefix: &str,
    out: &mut BTreeSet<String>,
) {
    if ids.contains("*") {
        return;
    }
    match value {
        Value::Array(entries) => {
            let prefix = format!("{prefix}[]");
            for entry in entries {
                undefined_fields(types, ids, entry, &prefix, out);
            }
        }
        Value::Object(fields) if !ids.is_empty() => {
            let nodes: Vec<&Map<String, Value>> = ids
                .iter()
                .map(|id| types[*id].as_object().expect("contract type"))
                .collect();
            for (key, entry) in fields {
                let name = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                let children: BTreeSet<&str> = nodes
                    .iter()
                    .filter_map(|node| node.get(key))
                    .flat_map(|ids| ids.as_array().expect("ids").iter())
                    .filter_map(Value::as_str)
                    .collect();
                if nodes.iter().all(|node| !node.contains_key(key)) {
                    if nodes.iter().all(|node| !node.contains_key("*")) {
                        out.insert(name);
                    }
                } else if !OPAQUE.contains(&key.as_str()) {
                    undefined_fields(types, &children, entry, &name, out);
                }
            }
        }
        _ => {}
    }
}

/// What the client side of `transcript` wrote that the contract does not
/// define: a kind it lacks, or a field none of the kind's types has.
pub(crate) fn client_violations(contract: &Value, transcript: &[(bool, Value)]) -> Vec<String> {
    let types = contract["types"].as_object().expect("contract types");
    let mut lines = BTreeSet::new();
    for ((out, kind), (_, frame)) in kinded(transcript).into_iter().zip(transcript) {
        if !out {
            continue;
        }
        let Some(ids) = contract["out"][&kind].as_array() else {
            lines.insert(format!("out {kind}: kind not in the protocol definitions"));
            continue;
        };
        let ids = ids.iter().filter_map(Value::as_str).collect();
        let mut fields = BTreeSet::new();
        undefined_fields(types, &ids, frame, "", &mut fields);
        lines.extend(
            fields
                .into_iter()
                .map(|field| format!("out {kind}: + {field}")),
        );
    }
    lines.into_iter().collect()
}

/// Fails when a replay of `harness`'s `scenario` wrote a frame its recorded
/// harness version's protocol definitions do not define.
pub(crate) fn assert_conforms(harness: &str, scenario: &str, transcript: &[(bool, Value)]) {
    let meta = std::fs::read_to_string(corpus(harness).join("meta.json")).expect("meta.json");
    let meta: Value = serde_json::from_str(&meta).expect("meta JSON");
    let version = meta["version"].as_str().expect("recorded version");
    let contract = contract_for(harness, version)
        .unwrap_or_else(|| panic!("{harness} {version}: no protocol contract at or below it"));
    let lines = client_violations(&contract, transcript);
    assert!(
        lines.is_empty(),
        "{harness}/{scenario}: Sage wrote frames {harness} {} does not define:\n{}",
        contract["version"],
        lines.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kinds named here are the kinds `shape.mjs` committed for the same
    /// recorded frames, so the Rust and Node checks key the same contract.
    #[test]
    fn frame_kinds_match_the_committed_corpus_shapes() {
        let root = corpus("claude").parent().expect("corpus root").to_owned();
        for entry in std::fs::read_dir(&root).expect("corpus root") {
            let harness = entry
                .expect("entry")
                .file_name()
                .into_string()
                .expect("name");
            let Ok(shape) = std::fs::read_to_string(root.join(&harness).join("shape.json")) else {
                continue;
            };
            let shape: Value = serde_json::from_str(&shape).expect("shape JSON");
            let mut kinds = BTreeSet::new();
            for scenario in super::super::wire_replay::scenarios(&harness) {
                let transcript: Vec<(bool, Value)> =
                    super::super::wire_replay::load(&harness, &scenario)
                        .into_iter()
                        .map(|record| (record.out, record.frame))
                        .collect();
                kinds.extend(
                    kinded(&transcript)
                        .into_iter()
                        .filter_map(|(out, kind)| (!out).then_some(kind)),
                );
            }
            let committed: BTreeSet<String> = shape["kinds"]
                .as_object()
                .expect("kinds")
                .keys()
                .cloned()
                .collect();
            assert_eq!(kinds, committed, "{harness}");
        }
    }

    #[test]
    fn a_field_or_kind_outside_the_definitions_is_reported() {
        let contract = serde_json::json!({
            "out": {"user": ["User"]},
            "types": {
                "User": {"type": [], "message": ["Message"], "meta": ["*"]},
                "Message": {"role": [], "content": ["Block"]},
                "Block": {"type": [], "text": [], "input": []}
            }
        });
        let frame = |value: Value| (true, value);
        let transcript = [
            frame(serde_json::json!({
                "type": "user",
                "meta": {"anything": 1},
                "message": {"role": "user", "content": [
                    {"type": "text", "text": "hi", "input": {"free": true}, "cache": 1}
                ]},
                "uuid": "u-1"
            })),
            frame(serde_json::json!({"type": "keep_alive"})),
            (false, serde_json::json!({"type": "assistant", "extra": 1})),
        ];
        assert_eq!(
            client_violations(&contract, &transcript),
            [
                "out keep_alive: kind not in the protocol definitions",
                "out user: + message.content[].cache",
                "out user: + uuid",
            ]
        );
    }
}
