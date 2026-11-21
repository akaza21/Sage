//! A single hidden operation on this app-server/account. Its wire identities
//! are consumed before the conversation translator sees any of its events.
use super::CodexRuntime;
use crate::acp::bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressToken};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

mod delivery;
mod isolation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Config,
    Start,
    Turn,
    Interrupt,
    Unsubscribe,
}

#[derive(Default)]
pub(super) struct TitleRequest {
    input: String,
    settings: Value,
    effort: Option<String>,
    config: Option<Value>,
    thread: Option<String>,
    turn: Option<String>,
    next: Option<Step>,
    pending: Vec<(u64, Step)>,
    ids: Vec<u64>,
    retired_threads: Vec<String>,
    attempts: u8,
    response: Option<String>,
    response_bytes: usize,
    pub(super) result: Option<Option<String>>,
    pub(super) cancelled: bool,
    pub(super) preempted: bool,
    interrupted: bool,
    unsubscribed: bool,
    pub(super) finished: bool,
    pub(super) cleanup_deadline: Option<Instant>,
}

impl TitleRequest {
    fn settle_reply(&mut self, id: u64) -> Option<Step> {
        let index = self
            .pending
            .iter()
            .position(|(pending, _)| *pending == id)?;
        // Keep the correlation ID so duplicate or late frames stay absorbed.
        Some(self.pending.remove(index).1)
    }

    pub(super) fn cancel(&mut self) {
        self.cancelled = true;
        if self.thread.is_none() && !self.pending.iter().any(|(_, step)| *step == Step::Start) {
            // Reads cannot allocate a remote thread. Retain only correlation IDs
            // so a late response is absorbed without starting auxiliary work.
            self.finish();
            return;
        }
        self.cleanup_deadline
            .get_or_insert_with(|| Instant::now() + Duration::from_secs(15));
    }

    fn finish(&mut self) {
        self.finished = true;
        self.input = String::new();
        self.response = None;
        self.settings = Value::Null;
        self.effort = None;
        self.config = None;
        self.pending.clear();
    }
}

impl CodexRuntime {
    pub(super) fn begin_title(&mut self, input: &str) -> Result<(), AdapterRuntimeError> {
        if input.len() > 4096
            || self
                .title
                .as_ref()
                .is_some_and(|t| !t.finished || t.attempts >= 3)
        {
            return Err(AdapterRuntimeError::Operation);
        }
        tracing::info!(
            settings_present = self.title_settings.is_some(),
            "Codex isolated title admission"
        );
        let mut settings = self
            .title_settings
            .clone()
            .ok_or(AdapterRuntimeError::Operation)?;
        let model = self
            .selected_model_id
            .as_ref()
            .map(|m| m.as_str())
            .or_else(|| settings["model"].as_str())
            .ok_or(AdapterRuntimeError::Operation)?;
        let model = if settings["modelProvider"] == "openai"
            && self.model_catalog.find("gpt-5.6-luna").is_some()
        {
            "gpt-5.6-luna"
        } else {
            model
        };
        let effort = self.model_catalog.find(model).and_then(|model| {
            model
                .supported_efforts
                .iter()
                .find(|effort| effort.id.as_str() == "low")
                .or_else(|| {
                    model
                        .supported_efforts
                        .iter()
                        .find(|effort| Some(&effort.id) == model.default_effort.as_ref())
                })
                .or_else(|| model.supported_efforts.first())
                .map(|effort| effort.id.as_str().to_owned())
        });
        settings["model"] = json!(model);
        let mut previous = self.title.take().unwrap_or_default();
        if let Some(thread) = previous.thread.take() {
            previous.retired_threads.push(thread);
        }
        self.title = Some(TitleRequest {
            input: input.to_owned(),
            settings,
            effort,
            next: Some(Step::Config),
            pending: Vec::with_capacity(4),
            // Three admitted tries, each with at most five wire requests.
            ids: previous.ids,
            retired_threads: previous.retired_threads,
            attempts: previous.attempts + 1,
            ..TitleRequest::default()
        });
        Ok(())
    }

    pub(super) fn next_title_frame(&mut self) -> Option<AdapterEgress> {
        let request = self.title.as_mut()?;
        if request.finished {
            return None;
        }
        let step = if request.cancelled && request.thread.is_some() {
            request.input = String::new();
            if request.turn.is_some() && !request.interrupted {
                request.interrupted = true;
                Some(Step::Interrupt)
            } else if request.pending.iter().any(|(_, step)| *step == Step::Turn) {
                None
            } else if !request.unsubscribed {
                request.unsubscribed = true;
                request.cancel();
                Some(Step::Unsubscribe)
            } else {
                None
            }
        } else if request.cancelled && request.pending.is_empty() {
            request.finish();
            None
        } else {
            request.next.take()
        }?;
        if request.pending.len() == 4 {
            request.cancelled = true;
            return None;
        }
        let params = match step {
            Step::Config => json!({"includeLayers":false,"cwd":request.settings["cwd"]}),
            Step::Start => {
                let params = isolation::thread_params(&request.settings, request.config.take()?);
                request.settings = Value::Null;
                params
            }
            Step::Turn => {
                let input = std::mem::take(&mut request.input);
                let mut params = json!({"threadId":request.thread,"input":[{"type":"text","text":input,"text_elements":[]}],
                    "outputSchema":{"type":"object","properties":{"title":{"type":"string"}},
                        "required":["title"],"additionalProperties":false}});
                if let Some(effort) = &request.effort {
                    params["effort"] = json!(effort);
                }
                params
            }
            Step::Interrupt => json!({"threadId":request.thread,"turnId":request.turn}),
            Step::Unsubscribe => json!({"threadId":request.thread}),
        };
        let method = match step {
            Step::Config => "config/read",
            Step::Start => "thread/start",
            Step::Turn => "turn/start",
            Step::Interrupt => "turn/interrupt",
            Step::Unsubscribe => "thread/unsubscribe",
        };
        let id = self.next_id().ok()?;
        tracing::info!(
            title_method = method,
            title_request_id = id,
            "Codex auxiliary title request"
        );
        let request = self.title.as_mut()?;
        request.pending.push((id, step));
        request.ids.push(id);
        let frame = crate::acp::transport::OutboundFrame::new(
            serde_json::to_vec(&json!({"id":id,"method":method,"params":params})).ok()?,
        )
        .ok()?;
        Some(AdapterEgress::new(
            frame,
            EgressToken::native_rpc_continuation(id),
        ))
    }

    pub(super) fn title_ingress(&mut self, value: &Value) -> bool {
        let Some(request) = &mut self.title else {
            return false;
        };
        let thread = value
            .pointer("/params/threadId")
            .or_else(|| value.pointer("/params/thread/id"))
            .and_then(Value::as_str);
        if thread.is_some_and(|thread| request.retired_threads.iter().any(|old| old == thread)) {
            return true;
        }
        if value.get("method").is_none()
            && let Some(id) = value["id"].as_u64()
            && request.ids.contains(&id)
        {
            let Some(step) = request.settle_reply(id) else {
                return true;
            };
            if value.get("error").is_some() {
                tracing::info!(
                    ?step,
                    code = value
                        .pointer("/error/code")
                        .and_then(serde_json::Value::as_i64),
                    "Codex auxiliary title declined"
                );
                request.cancel();
                request.result = Some(None);
                if step == Step::Unsubscribe {
                    request.cleanup_deadline = Some(Instant::now());
                } else if step == Step::Config || (step == Step::Start && request.thread.is_none())
                {
                    request.finish();
                }
                return true;
            }
            match step {
                Step::Config => {
                    request.config =
                        isolation::disabled_config(value, self.mcp.codex_config().ok().flatten());
                    request.next = request.config.as_ref().map(|_| Step::Start);
                    tracing::info!(
                        admitted = request.config.is_some(),
                        "Codex title isolation configuration read"
                    );
                    if request.config.is_none() {
                        request.cancelled = true;
                    }
                }
                Step::Start => {
                    request.thread =
                        bounded_text(value, "/result/thread/id", 512).filter(|id| !id.is_empty());
                    tracing::info!(
                        title_thread_id = request.thread.as_deref(),
                        sandbox = value
                            .pointer("/result/sandbox/type")
                            .and_then(serde_json::Value::as_str),
                        "Codex temporary title thread started"
                    );
                    if request.thread.is_some()
                        && value
                            .pointer("/result/sandbox/type")
                            .and_then(serde_json::Value::as_str)
                            == Some("readOnly")
                    {
                        request.next = Some(Step::Turn);
                    } else {
                        request.cancelled = true;
                    }
                }
                Step::Turn => {
                    request.turn =
                        bounded_text(value, "/result/turn/id", 512).filter(|id| !id.is_empty());
                    if request.turn.is_none() {
                        request.cancelled = true;
                    }
                }
                Step::Interrupt => {}
                Step::Unsubscribe => {
                    tracing::info!(
                        status = value
                            .pointer("/result/status")
                            .and_then(serde_json::Value::as_str),
                        "Codex temporary title unsubscribe answered"
                    );
                    // Keep global admission through Codex's unload grace period.
                    // thread/closed releases it earlier when the supervisor confirms unload.
                    if value.pointer("/result/status").and_then(Value::as_str) == Some("notLoaded")
                    {
                        request.finish();
                    } else {
                        request.cancel();
                    }
                }
            }
            return true;
        }
        let method = value["method"].as_str().unwrap_or_default();
        let params = &value["params"];
        // thread/started can precede the request response. An ephemeral feature
        // thread cannot be a child conversation, even before its ID is known.
        if method == "thread/started" && params.pointer("/thread/ephemeral") == Some(&json!(true)) {
            if request.thread.is_none() {
                request.thread = bounded_text(params, "/thread/id", 512);
            }
            return true;
        }
        let thread = params["threadId"].as_str().or_else(|| {
            params
                .pointer("/thread/id")
                .and_then(serde_json::Value::as_str)
        });
        if thread.is_none() || thread != request.thread.as_deref() {
            return false;
        }
        if value.get("id").is_some() {
            request.cancelled = true;
        }
        match method {
            "item/completed"
                if params
                    .pointer("/item/type")
                    .and_then(serde_json::Value::as_str)
                    == Some("agentMessage") =>
            {
                request.response = bounded_text(params, "/item/text", 1024);
                if request.response.is_none() {
                    request.cancelled = true;
                }
            }
            "item/agentMessage/delta" => {
                request.response_bytes = request
                    .response_bytes
                    .saturating_add(params["delta"].as_str().map_or(0, str::len));
                if request.response_bytes > 1024 {
                    request.cancelled = true;
                }
            }
            "turn/completed" => {
                let completed = params
                    .pointer("/turn/status")
                    .and_then(serde_json::Value::as_str)
                    == Some("completed");
                request.result = if request.preempted
                    && params
                        .pointer("/turn/status")
                        .and_then(Value::as_str)
                        .is_some_and(|status| matches!(status, "interrupted" | "cancelled"))
                {
                    None
                } else {
                    Some(
                        if completed
                            && (!request.cancelled || request.preempted)
                            && request.response_bytes <= 1024
                        {
                            request
                                .response
                                .take()
                                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                                .and_then(|value| value["title"].as_str().map(str::to_owned))
                        } else {
                            None
                        },
                    )
                };
                request.cancelled = true;
                request.interrupted = true;
            }
            "thread/closed" => {
                request.finish();
            }
            _ => {}
        }
        true
    }
}

fn bounded_text(value: &Value, path: &str, max_bytes: usize) -> Option<String> {
    value
        .pointer(path)?
        .as_str()
        .filter(|text| text.len() <= max_bytes)
        .map(str::to_owned)
}
