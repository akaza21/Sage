//! Replays a real harness transcript recorded by
//! `scripts/harness-wire/capture.mjs` through a binding and projects what the
//! binding made of it. The driver knows no harness: each recorded client
//! frame that starts a Sage command carries a `step`, the matching
//! `AdapterCommand` is issued, recorded request ids are re-pointed at the ids
//! the binding chose, and every harness frame goes through `ingest`.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::Arc,
};

use base64::Engine as _;
use serde_json::{Value, json};

use super::adapter::{
    AdapterCommand, AdapterEgress, AdapterEvent, EgressResolution, HarnessAdapter, ResourceMapper,
};
use crate::acp::{
    attached_context::files::images::{ImagePreview, PromptImage, PromptImages},
    transcript_quote::Prompt,
    transport::{Frame, FrameReader},
};
use sage_acp_protocol::{
    access_mode::SessionAccessMode, contract::RequiredText, permissions::PermissionDecision,
};

/// Where the committed corpus lives.
pub(crate) fn corpus(harness: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/harness-wire")
        .join(harness)
}

/// The sanitized scratch repository every transcript works in.
pub(crate) const ROOT: &str = "C:\\fixture-workspace\\repo";

/// Maps the scratch repository's files to one Sage root.
pub(crate) struct FixtureMapper;

impl ResourceMapper for FixtureMapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<
        sage_acp_protocol::intent::SafeResourceRef,
        sage_acp_protocol::intent::UnavailableResourceReason,
    > {
        let path = absolute_path.replace('/', "\\");
        path.strip_prefix(ROOT)
            .and_then(|rest| rest.strip_prefix('\\'))
            .and_then(|relative| {
                sage_acp_protocol::intent::SafeResourceRef::new(
                    sage_acp_protocol::contract::SageRootId::parse("root-replay").ok()?,
                    &relative.replace('\\', "/"),
                )
                .ok()
            })
            .ok_or(sage_acp_protocol::intent::UnavailableResourceReason::Foreign)
    }
}

/// The scenarios a harness's corpus holds, so a new capture is replayed
/// without a test naming it.
pub(crate) fn scenarios(harness: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(corpus(harness))
        .expect("corpus")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            name.strip_suffix(".jsonl").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

pub(crate) struct Record {
    out: bool,
    step: Option<Value>,
    frame: Value,
}

fn lines(harness: &str, file: &str) -> Vec<Value> {
    let path = corpus(harness).join(file);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("transcript line"))
        .collect()
}

/// The scenario's records; a `ref` record (`commands.jsonl#<line>`) stands
/// for the frame that line of the same corpus holds.
pub(crate) fn load(harness: &str, scenario: &str) -> Vec<Record> {
    lines(harness, &format!("{scenario}.jsonl"))
        .into_iter()
        .map(|record| {
            let frame = match record["ref"].as_str() {
                Some(reference) => {
                    let (file, at) = reference.split_once('#').expect("ref names a line");
                    lines(harness, file)[at.parse::<usize>().expect("line")]["frame"].clone()
                }
                None => record["frame"].clone(),
            };
            Record {
                out: record["direction"] == "out",
                step: record.get("step").cloned(),
                frame,
            }
        })
        .collect()
}

pub(crate) fn frame(value: &Value) -> Frame {
    let mut bytes = serde_json::to_vec(value).expect("frame bytes");
    bytes.push(b'\n');
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
        .block_on(FrameReader::new(bytes.as_slice()).read_frame())
        .expect("frame reads")
        .expect("one frame")
}

/// What a client frame asks for, in any of the three wires: a JSON-RPC
/// method, a stream-json control subtype, a Pi command or message type.
fn out_kind(frame: &Value) -> String {
    let text = |value: &Value| value.as_str().map(str::to_owned);
    text(&frame["method"])
        .or_else(|| text(&frame["request"]["subtype"]))
        .or_else(|| text(&frame["type"]))
        .unwrap_or_else(|| "reply".into())
}

fn out_id(frame: &Value) -> Option<Value> {
    ["/request_id", "/id"]
        .iter()
        .find_map(|pointer| frame.pointer(pointer).filter(|id| !id.is_null()).cloned())
}

/// A harness frame that answers a client request, so its id is the client's.
fn is_reply(frame: &Value) -> bool {
    frame.get("method").is_none()
        && matches!(
            frame["type"].as_str(),
            None | Some("control_response" | "response")
        )
}

fn first_png(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::String(text) => {
            let data = text.rsplit("base64,").next()?;
            data.starts_with("iVBOR")
                .then(|| base64::engine::general_purpose::STANDARD.decode(data).ok())
                .flatten()
        }
        Value::Array(list) => list.iter().find_map(first_png),
        Value::Object(map) => map.values().find_map(first_png),
        _ => None,
    }
}

#[derive(Default)]
pub(crate) struct Replay {
    pub(crate) events: Vec<AdapterEvent>,
    pub(crate) refused: Vec<String>,
    pub(crate) ingest_errors: Vec<String>,
    live: VecDeque<Value>,
    ids: HashMap<String, Value>,
    client_only: Vec<String>,
    resolved: usize,
}

impl Replay {
    fn written(&mut self, adapter: &mut dyn HarnessAdapter, egress: AdapterEgress) {
        let sent: Value = serde_json::from_slice(egress.frame().payload()).expect("egress JSON");
        self.live.push_back(sent);
        // As the adapter pump writes it: committed if mandatory, queued, then
        // written.
        if egress.token().is_mandatory() {
            adapter
                .commit_mandatory_egress(egress.token())
                .expect("mandatory egress commits");
        }
        adapter
            .resolve_egress(egress.token().clone(), EgressResolution::Queued)
            .expect("egress queues");
        adapter
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .expect("egress is written");
    }

    fn command_for(&mut self, step: &Value, out: &Value) -> Option<AdapterCommand> {
        let text = |key: &str| step[key].as_str().unwrap_or_default().to_owned();
        match step["command"].as_str()? {
            "initialize" => Some(AdapterCommand::Initialize),
            "session" => Some(AdapterCommand::NewSession {
                native_root: ROOT.into(),
                model: None,
            }),
            "prompt" => {
                let mut prompt = Prompt::new("prompt", &text("text")).expect("prompt");
                if let Some(bytes) = first_png(out) {
                    prompt.images = PromptImages(Arc::new(vec![PromptImage {
                        media_type: sage_acp_protocol::intent::ImageMediaType::Png,
                        bytes,
                        preview: ImagePreview::Full,
                    }]));
                }
                Some(AdapterCommand::Prompt {
                    prompt,
                    client_message_id: RequiredText::new("client", "replay-turn").expect("id"),
                    access_mode: if step["mode"] == "plan" {
                        SessionAccessMode::Plan
                    } else {
                        SessionAccessMode::Ask
                    },
                })
            }
            "permission" => {
                // The oldest permission the replay has not answered yet.
                let request = self
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        AdapterEvent::PermissionRequest {
                            external_request_id,
                            ..
                        } => Some(external_request_id.as_str().to_owned()),
                        _ => None,
                    })
                    .nth(self.resolved)?;
                self.resolved += 1;
                Some(AdapterCommand::ResolvePermission {
                    request_id: sage_acp_protocol::contract::SageServerRequestId::parse(&request)
                        .expect("request id"),
                    decision: if step["decision"] == "deny" {
                        PermissionDecision::Deny
                    } else {
                        PermissionDecision::AllowOnce
                    },
                })
            }
            _ => None,
        }
    }

    fn client(&mut self, adapter: &mut dyn HarnessAdapter, record: &Record) {
        if let Some(step) = &record.step
            && let Some(command) = self.command_for(step, &record.frame)
        {
            match adapter.command(command) {
                Ok(egress) => self.written(adapter, egress),
                Err(error) => self.refused.push(format!("{} {error}", step["command"])),
            }
        }
        let kind = out_kind(&record.frame);
        match self.live.iter().position(|sent| out_kind(sent) == kind) {
            Some(index) => {
                let sent = self.live.remove(index).expect("matched egress");
                if let (Some(recorded), Some(live)) = (out_id(&record.frame), out_id(&sent)) {
                    self.ids.insert(recorded.to_string(), live);
                }
            }
            None => self.client_only.push(kind),
        }
    }

    fn harness(
        &mut self,
        adapter: &mut dyn HarnessAdapter,
        mut value: Value,
        resources: &dyn ResourceMapper,
    ) {
        if is_reply(&value) {
            for pointer in ["/id", "/response/request_id"] {
                if let Some(slot) = value.pointer_mut(pointer)
                    && let Some(live) = self.ids.get(&slot.to_string())
                {
                    *slot = live.clone();
                }
            }
        }
        let mut batch = Some(adapter.ingest(&frame(&value), resources));
        while let Some(result) = batch.take() {
            match result {
                Ok(batch) => {
                    let (events, egress) = batch.into_parts();
                    self.events.extend(events);
                    for egress in egress {
                        self.written(adapter, egress);
                    }
                }
                Err(error) => self.ingest_errors.push(error.to_string()),
            }
            batch = adapter.held_batch().transpose();
        }
    }

    /// Feeds `records` to `adapter` and ends as the harness's output ends.
    pub(crate) fn run(
        adapter: &mut dyn HarnessAdapter,
        records: &[Record],
        resources: &dyn ResourceMapper,
    ) -> Self {
        let mut replay = Self::default();
        for record in records {
            if record.out {
                replay.client(adapter, record);
            } else {
                replay.harness(adapter, record.frame.clone(), resources);
            }
        }
        match adapter.unfinished_replies() {
            Ok(events) => replay.events.extend(events),
            Err(error) => replay.ingest_errors.push(error.to_string()),
        }
        replay
    }

    /// The projection a scenario commits: what the binding listed, the rows
    /// and states it produced, and every refusal, error and unsupported frame.
    pub(crate) fn summary(&self, adapter: &dyn HarnessAdapter) -> Value {
        let commands: Vec<String> = adapter
            .available_commands()
            .snapshot()
            .commands
            .iter()
            .map(|command| command.name.clone())
            .collect();
        let mut rows: Vec<(String, usize)> = Vec::new();
        for event in &self.events {
            let row = row(event);
            match rows.last_mut() {
                Some((last, count)) if *last == row => *count += 1,
                _ => rows.push((row, 1)),
            }
        }
        json!({
            "commands": commands,
            "refused": self.refused,
            "ingestErrors": self.ingest_errors,
            "clientOnly": self.client_only,
            "sageOnly": self.live.iter().map(out_kind).collect::<Vec<_>>(),
            "rows": rows
                .into_iter()
                .map(|(row, count)| if count == 1 { row } else { format!("{row} x{count}") })
                .collect::<Vec<_>>(),
        })
    }
}

/// Keys whose values are prose, ids or bulk the projection leaves out: the
/// rows state what kind of thing happened, not what the model wrote.
const DROPPED: [&str; 24] = [
    "id",
    "itemId",
    "item_id",
    "turnId",
    "turn_id",
    "externalRequestId",
    "external_request_id",
    "externalItemId",
    "external_item_id",
    "requestPayloadDigest",
    "request_payload_digest",
    "childThreadId",
    "child_thread_id",
    "childTurnId",
    "child_turn_id",
    "observedSequence",
    "text",
    "summary",
    "inputSummary",
    "outputSummary",
    "old_text",
    "new_text",
    "details",
    "capabilities",
];

fn prune(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !DROPPED.contains(&key.as_str()));
            map.values_mut().for_each(prune);
        }
        Value::Array(list) => list.iter_mut().for_each(prune),
        Value::String(text) if text.len() > 48 => {
            let cut = text.char_indices().nth(48).map_or(text.len(), |(at, _)| at);
            text.truncate(cut);
            text.push('…');
        }
        _ => {}
    }
}

fn row(event: &AdapterEvent) -> String {
    let mut value = serde_json::to_value(event).expect("event JSON");
    prune(&mut value);
    match value {
        Value::String(name) => name,
        Value::Object(map) => map
            .into_iter()
            .map(|(name, body)| format!("{name} {body}"))
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    }
}

/// Compares a replay's summary with the committed `<scenario>.replay.json`.
/// `SAGE_WIRE_BLESS=1` rewrites it instead, after a reviewed recapture.
pub(crate) fn assert_summary(harness: &str, scenario: &str, summary: &Value) {
    let path = corpus(harness).join(format!("{scenario}.replay.json"));
    let rendered = format!("{}\n", serde_json::to_string_pretty(summary).expect("summary"));
    if std::env::var_os("SAGE_WIRE_BLESS").is_some() {
        std::fs::write(&path, rendered).expect("bless");
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{path:?}: {error}; bless a reviewed capture"));
    let committed: Value = serde_json::from_str(&committed).expect("committed summary");
    assert_eq!(
        &committed, summary,
        "{harness}/{scenario}: the binding no longer projects the recorded transcript the same way"
    );
}
