//! A single hidden operation on this app-server/account. Its wire identities
//! are consumed before the conversation translator sees any of its events.
use super::CodexRuntime;
use crate::acp::bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressToken};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

mod isolation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Config,
    Start,
    Turn,
    Interrupt,
    Unsubscribe,
}

pub(super) struct TitleRequest {
    input: String,
    settings: Value,
    config: Option<Value>,
    thread: Option<String>,
    turn: Option<String>,
    next: Option<Step>,
    pending: Vec<(u64, Step)>,
    ids: Vec<u64>,
    response: Option<String>,
    response_bytes: usize,
    pub(super) result: Option<Option<String>>,
    pub(super) cancelled: bool,
    interrupted: bool,
    unsubscribed: bool,
    pub(super) finished: bool,
    pub(super) cleanup_deadline: Option<Instant>,
}

impl CodexRuntime {
    pub(super) fn begin_title(&mut self, input: &str) -> Result<(), AdapterRuntimeError> {
        if input.len() > 4096 || self.title.as_ref().is_some_and(|t| !t.finished) {
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
        settings["model"] = json!(model);
        self.title = Some(TitleRequest {
            input: input.to_owned(),
            settings,
            config: None,
            thread: None,
            turn: None,
            next: Some(Step::Config),
            pending: Vec::with_capacity(4),
            ids: Vec::with_capacity(5),
            response: None,
            response_bytes: 0,
            result: None,
            cancelled: false,
            interrupted: false,
            unsubscribed: false,
            finished: false,
            cleanup_deadline: None,
        });
        Ok(())
    }

    pub(super) fn next_title_frame(&mut self) -> Option<AdapterEgress> {
        let request = self.title.as_mut()?;
        if request.finished {
            return None;
        }
        if request
            .cleanup_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            request.input = String::new();
            request.response = None;
            request.settings = Value::Null;
            request.config = None;
            // A timer is not evidence of unload. Keep the bounded operation
            // and global lease until thread/closed or supervisor-proven exit.
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
                request
                    .cleanup_deadline
                    .get_or_insert_with(|| Instant::now() + Duration::from_secs(15));
                Some(Step::Unsubscribe)
            } else {
                None
            }
        } else if request.cancelled && request.pending.is_empty() {
            request.finished = true;
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
                json!({"threadId":request.thread,"input":[{"type":"text","text":input,"text_elements":[]}],
                    "effort":"low","outputSchema":{"type":"object","properties":{"title":{"type":"string"}},
                        "required":["title"],"additionalProperties":false}})
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
        if value.get("method").is_none()
            && let Some(id) = value["id"].as_u64()
            && request.ids.contains(&id)
        {
            let Some(index) = request
                .pending
                .iter()
                .position(|(pending, _)| *pending == id)
            else {
                return true;
            };
            let (_, step) = request.pending.remove(index);
            if value.get("error").is_some() {
                tracing::info!(
                    ?step,
                    code = value
                        .pointer("/error/code")
                        .and_then(serde_json::Value::as_i64),
                    "Codex auxiliary title declined"
                );
                request.cancelled = true;
                request.result = Some(None);
                if step == Step::Unsubscribe {
                    request.cleanup_deadline = Some(Instant::now());
                } else if step == Step::Config || (step == Step::Start && request.thread.is_none())
                {
                    request.finished = true;
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
                    request.thread = value
                        .pointer("/result/thread/id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty() && id.len() <= 512)
                        .map(str::to_owned);
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
                    request.turn = value
                        .pointer("/result/turn/id")
                        .and_then(serde_json::Value::as_str)
                        .filter(|id| !id.is_empty() && id.len() <= 512)
                        .map(str::to_owned);
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
                    request
                        .cleanup_deadline
                        .get_or_insert_with(|| Instant::now() + Duration::from_secs(15));
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
                request.thread = params
                    .pointer("/thread/id")
                    .and_then(serde_json::Value::as_str)
                    .filter(|id| id.len() <= 512)
                    .map(str::to_owned);
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
                request.response = params
                    .pointer("/item/text")
                    .and_then(serde_json::Value::as_str)
                    .filter(|text| text.len() <= 1024)
                    .map(str::to_owned);
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
                request.result = Some(if completed && !request.cancelled {
                    request
                        .response
                        .take()
                        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
                        .and_then(|value| value["title"].as_str().map(str::to_owned))
                } else {
                    None
                });
                request.cancelled = true;
                request.interrupted = true;
            }
            "thread/closed" => {
                request.finished = true;
            }
            _ => {}
        }
        true
    }

    pub(super) fn title_write(
        &mut self,
        id: u64,
        resolution: crate::acp::bindings::adapter::EgressResolution,
    ) -> bool {
        let Some(request) = &mut self.title else {
            return false;
        };
        if !request.ids.contains(&id) {
            return false;
        }
        use crate::acp::bindings::adapter::EgressResolution::*;
        if matches!(resolution, QueueRejected | WriteZeroBytes | DeliveryUnknown) {
            request.cancelled = true;
            request
                .cleanup_deadline
                .get_or_insert_with(|| Instant::now() + Duration::from_secs(15));
        }
        true
    }
}
