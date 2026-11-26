//! Drives a binding through a recorded transcript the way the adapter pump
//! drives it live: commands at the recorded steps, egress committed, queued
//! and written, held batches drained, a harness-started Turn adopted, the
//! answers to a permission reported delivered, and the idle reads (command
//! list, model catalog) written once nothing runs.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
};

use base64::Engine as _;
use serde_json::{Value, json};

use super::{ROOT, Record, frame, projection};
use crate::acp::{
    attached_context::files::images::{ImagePreview, PromptImage, PromptImages},
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterRuntimeError,
        EgressResolution, HarnessAdapter, ResourceMapper,
    },
    transcript_quote::Prompt,
};
use sage_acp_protocol::{
    access_mode::SessionAccessMode, contract::RequiredText, permissions::PermissionDecision,
};

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

const REPLY_IDS: [&str; 2] = ["/id", "/response/request_id"];

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
    /// The Sage session the runtime was built for; a mode change names it.
    session: String,
    events: Vec<AdapterEvent>,
    refused: Vec<String>,
    ingest_errors: Vec<String>,
    /// What the binding wrote that no recorded client frame has matched yet.
    live: VecDeque<Value>,
    /// Recorded request ids, by the id the binding chose for the same request.
    ids: HashMap<String, Value>,
    /// Recorded requests the binding had not written when they were sent;
    /// paired when their reply arrives, if the binding has written one since.
    unpaired: Vec<(String, String)>,
    answered: HashSet<String>,
    turns: usize,
}

impl Replay {
    fn written(&mut self, adapter: &mut dyn HarnessAdapter, egress: AdapterEgress) {
        let sent: Value = serde_json::from_slice(egress.frame().payload()).expect("egress JSON");
        self.live.push_back(sent);
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

    /// Idle work the pump writes straight to the transport.
    fn idle_write(&mut self, adapter: &mut dyn HarnessAdapter, egress: AdapterEgress) {
        let sent = serde_json::from_slice(egress.frame().payload()).expect("JSON");
        self.live.push_back(sent);
        adapter
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .expect("idle read is written");
    }

    fn permission(&mut self, step: &Value) -> Option<AdapterCommand> {
        // The oldest request still open: neither answered here nor settled
        // by the harness.
        let settled: HashSet<&str> = self
            .events
            .iter()
            .filter_map(|event| match event {
                AdapterEvent::PermissionResolved {
                    external_request_id,
                    ..
                } => Some(external_request_id.as_str()),
                _ => None,
            })
            .collect();
        let request = self.events.iter().find_map(|event| match event {
            AdapterEvent::PermissionRequest {
                external_request_id,
                ..
            } if !settled.contains(external_request_id.as_str())
                && !self.answered.contains(external_request_id.as_str()) =>
            {
                Some(external_request_id.as_str().to_owned())
            }
            _ => None,
        })?;
        self.answered.insert(request.clone());
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

    fn command_for(&mut self, step: &Value, out: &Value) -> Option<AdapterCommand> {
        match step["command"].as_str()? {
            "initialize" => Some(AdapterCommand::Initialize),
            "session" => Some(AdapterCommand::NewSession {
                native_root: ROOT.into(),
                model: None,
            }),
            "prompt" => {
                let text = step["text"].as_str().unwrap_or_default();
                let mut prompt = Prompt::new("prompt", text).expect("prompt");
                if let Some(bytes) = first_png(out) {
                    prompt.images = PromptImages(Arc::new(vec![PromptImage {
                        media_type: sage_acp_protocol::intent::ImageMediaType::Png,
                        bytes,
                        preview: ImagePreview::Full,
                    }]));
                }
                self.turns += 1;
                let id = format!("replay-turn-{}", self.turns);
                Some(AdapterCommand::Prompt {
                    prompt,
                    client_message_id: RequiredText::new("client", id).expect("id"),
                    access_mode: if step["mode"] == "plan" {
                        SessionAccessMode::Plan
                    } else {
                        SessionAccessMode::Ask
                    },
                })
            }
            "permission" => self.permission(step),
            _ => None,
        }
    }

    /// A session mode change, as Sage sends one before a prompt in that mode.
    fn configure(&mut self, adapter: &mut dyn HarnessAdapter, mode: &str) {
        use sage_acp_protocol::{
            contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
            scheduler::RuntimeOwner,
            session_config::{ConfigId, ConfigIntent, ConfigMutation},
        };
        let intent = ConfigIntent {
            owner: RuntimeOwner {
                project_id: SageProjectId::parse("project-replay").expect("project"),
                chat_id: SageChatId::parse("chat-replay").expect("chat"),
                session_id: SageAgentSessionId::parse(&self.session).expect("session"),
                runtime_epoch: RuntimeEpoch::new(1).expect("epoch"),
            },
            expected_config_revision: serde_json::from_value(json!("1")).expect("revision"),
            mutation: ConfigMutation::Mode(ConfigId::new("mode", mode).expect("mode id")),
        };
        match adapter.configure(&intent) {
            Ok(Some(egress)) => self.written(adapter, egress),
            Ok(None) => {}
            Err(error) => self.refused.push(format!("mode {mode} {error}")),
        }
    }

    fn client(
        &mut self,
        adapter: &mut dyn HarnessAdapter,
        record: &Record,
        resources: &dyn ResourceMapper,
    ) {
        if let Some(step) = &record.step {
            if step["command"] == "mode" {
                self.configure(adapter, step["mode"].as_str().unwrap_or_default());
            } else if let Some(command) = self.command_for(step, &record.frame) {
                let answer = matches!(command, AdapterCommand::ResolvePermission { .. });
                match adapter.command(command) {
                    Ok(egress) => {
                        self.written(adapter, egress);
                        // The pump reports a written answer delivered: a Turn
                        // that only waited on it may end.
                        if answer
                            && let Some(batch) = adapter.answers_delivered(resources).transpose()
                        {
                            self.batches(adapter, batch);
                        }
                    }
                    Err(error) => self.refused.push(format!("{} {error}", step["command"])),
                }
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
            None => {
                let id = out_id(&record.frame).map_or_else(String::new, |id| id.to_string());
                self.unpaired.push((kind, id));
            }
        }
    }

    /// Pairs a reply to a request the binding wrote later than the capture
    /// client did, as long as it has written one of the same kind since.
    fn pair_late(&mut self, recorded: &str) {
        let Some(at) = self.unpaired.iter().position(|(_, id)| id == recorded) else {
            return;
        };
        let kind = &self.unpaired[at].0;
        if let Some(index) = self.live.iter().position(|sent| &out_kind(sent) == kind) {
            let sent = self.live.remove(index).expect("late egress");
            if let Some(live) = out_id(&sent) {
                self.ids.insert(recorded.to_owned(), live);
            }
            self.unpaired.remove(at);
        }
    }

    fn harness(
        &mut self,
        adapter: &mut dyn HarnessAdapter,
        mut value: Value,
        resources: &dyn ResourceMapper,
    ) {
        if is_reply(&value) {
            for pointer in REPLY_IDS {
                if let Some(slot) = value.pointer_mut(pointer) {
                    let recorded = slot.to_string();
                    if !self.ids.contains_key(&recorded) {
                        self.pair_late(&recorded);
                    }
                    if let Some(live) = self.ids.get(&recorded) {
                        *slot = live.clone();
                    }
                }
            }
        }
        let start = self.events.len();
        let result = adapter.ingest(&frame(&value), resources);
        self.batches(adapter, result);
        let new = &self.events[start..];
        let started = new
            .iter()
            .any(|event| matches!(event, AdapterEvent::TurnStarted { .. }));
        let idle = new.iter().any(|event| match event {
            AdapterEvent::SessionReady { .. } => true,
            AdapterEvent::Turn { state, .. } => state.is_terminal(),
            _ => false,
        });
        if started {
            self.turns += 1;
            let id = format!("replay-harness-turn-{}", self.turns);
            let result =
                adapter.adopt_turn(RequiredText::new("client", id).expect("id"), resources);
            self.batches(adapter, result);
        }
        if idle {
            match adapter.discover_commands() {
                Ok(Some(egress)) => self.idle_write(adapter, egress),
                Ok(None) => {}
                Err(error) => self.refused.push(format!("discover commands {error}")),
            }
            match adapter.model_config_hydration() {
                Ok(Some(egress)) => self.idle_write(adapter, egress),
                Ok(None) => {}
                Err(error) => self.refused.push(format!("model catalog {error}")),
            }
        }
    }

    /// Takes a batch and every batch held back behind it.
    fn batches(
        &mut self,
        adapter: &mut dyn HarnessAdapter,
        first: Result<AdapterBatch, AdapterRuntimeError>,
    ) {
        let mut next = Some(first);
        while let Some(result) = next.take() {
            match result {
                Ok(batch) => {
                    let (events, egress) = batch.into_parts();
                    self.events.extend(events);
                    for egress in egress {
                        self.written(adapter, egress);
                    }
                    next = adapter.held_batch().transpose();
                }
                Err(error) => self.ingest_errors.push(error.to_string()),
            }
        }
    }

    /// Feeds `records` to `adapter`, built for Sage session `session` at
    /// epoch 1, and ends as the harness's output ends.
    pub(crate) fn run(
        adapter: &mut dyn HarnessAdapter,
        session: &str,
        records: &[Record],
        resources: &dyn ResourceMapper,
    ) -> Self {
        let mut replay = Self {
            session: session.to_owned(),
            ..Self::default()
        };
        for record in records {
            if record.out {
                replay.client(adapter, record, resources);
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
    /// it produced, and every refusal, error and unmatched frame either way.
    pub(crate) fn summary(&self, adapter: &dyn HarnessAdapter) -> Value {
        projection::summary(
            adapter,
            &self.events,
            json!({
                "refused": self.refused,
                "ingestErrors": self.ingest_errors,
                "clientOnly": self.unpaired.iter().map(|(kind, _)| kind).collect::<Vec<_>>(),
                "sageOnly": self.live.iter().map(out_kind).collect::<Vec<_>>(),
            }),
        )
    }
}
