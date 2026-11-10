use super::super::FixtureError;
use super::{WireState, codex};
use serde_json::{Value, json};
use std::io::{Read, Write};

#[path = "wire_acp_resources.rs"]
mod resources;
pub(super) use resources::EmbeddedContext;

pub(super) const SESSION: &str = "fixture-persisted-session-1";
const ELICITATION_ID: &str = "fixture-elicitation-1";

pub(super) fn await_request(
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
    kind: &str,
    prompt_text: Option<&str>,
) -> Result<(), FixtureError> {
    if prompt_text.is_some() && kind != "acpPrompt" {
        return Err(FixtureError::InvalidArguments);
    }
    let expected = match kind {
        "acpInitialize" => "initialize",
        "acpNew" => "session/new",
        // A journey that runs across an app restart: a new session first,
        // the same session resumed after.
        "acpOpen" => "session/new|session/resume",
        "acpPrompt" => "session/prompt",
        "acpCancel" => "session/cancel",
        "acpClose" => "session/close",
        "acpPermissionDecision" => "permission",
        "acpFork" => "session/fork",
        "acpAuthenticate" => "authenticate",
        "acpElicitationReply" => "elicitation",
        _ => return Err(FixtureError::InvalidArguments),
    };
    for _ in 0..32 {
        let Some(request) = codex::read_json_line(input, state)? else {
            return if expected == "session/close" {
                Ok(())
            } else {
                Err(FixtureError::Io)
            };
        };
        let method = request.get("method").and_then(Value::as_str);
        if matches!(
            method,
            Some("session/set_mode" | "session/set_config_option")
        ) {
            let id = request
                .get("id")
                .and_then(Value::as_u64)
                .ok_or(FixtureError::InvalidArguments)?;
            if let Some(config) = state.acp_config.as_mut() {
                let frames = config.answer(id, &request);
                for frame in frames {
                    codex::emit_json(output, state, &frame)?;
                }
                continue;
            }
            let result = if method == Some("session/set_config_option") {
                json!({"configOptions":config_options()})
            } else {
                json!({})
            };
            codex::emit_json(
                output,
                state,
                &json!({"jsonrpc":"2.0","id":id,"result":result}),
            )?;
            continue;
        }
        if expected == "elicitation" {
            state.user_input_echo = Some(elicitation_echo(&request)?);
            return Ok(());
        }
        if expected == "permission" {
            if request.get("id") != Some(&json!("fixture-permission-1"))
                || request.get("method").is_some()
            {
                return Err(FixtureError::InvalidArguments);
            }
            let outcome = &request["result"]["outcome"];
            if outcome["outcome"] == "selected"
                && !matches!(
                    outcome["optionId"].as_str(),
                    Some("fixture-allow-once" | "fixture-allow-always" | "fixture-reject")
                )
            {
                return Err(FixtureError::InvalidArguments);
            }
            if !matches!(outcome["outcome"].as_str(), Some("selected" | "cancelled")) {
                return Err(FixtureError::InvalidArguments);
            }
            return Ok(());
        }
        if !method.is_some_and(|method| expected.split('|').any(|one| one == method)) {
            return Err(FixtureError::InvalidArguments);
        }
        if method == Some("session/resume")
            && request.pointer("/params/sessionId") != Some(&json!(SESSION))
        {
            return Err(FixtureError::InvalidArguments);
        }
        if expected == "session/cancel" {
            if request.get("id").is_some() {
                return Err(FixtureError::InvalidArguments);
            }
            return Ok(());
        }
        let id = request
            .get("id")
            .and_then(Value::as_u64)
            .ok_or(FixtureError::InvalidArguments)?;
        state.last_request_id = Some(id);
        if expected == "session/prompt" {
            // A journey that names the prompt text proves what Sage put on
            // the wire: one text block, byte for byte.
            if let Some(text) = prompt_text
                && request["params"]["prompt"] != json!([{"type":"text","text":text}])
            {
                return Err(FixtureError::InvalidArguments);
            }
            if let Some(embedded) = state.embedded_context.as_mut() {
                embedded.receive(&request["params"]["prompt"])?;
            }
            state.prompt_echo = request["params"]["prompt"]
                .as_array()
                .map(|blocks| super::prompt_echo::blocks(blocks));
            state.prompt_request_id = Some(id);
        }
        return Ok(());
    }
    Err(FixtureError::ExpandedLimit)
}

/// The session config of a journey that changes model, effort and mode: it
/// answers each change with the new value, as claude-agent-acp does, and a
/// new or resumed session starts again from the adapter's defaults.
pub(super) struct AcpConfig {
    model: &'static str,
    effort: &'static str,
    mode: &'static str,
}

const MODELS: [&str; 2] = ["fixture-model", "fixture-model-deep"];
const EFFORTS: [&str; 3] = ["low", "medium", "high"];
const MODES: [&str; 3] = ["default", "acceptEdits", "plan"];

impl AcpConfig {
    fn defaults() -> Self {
        Self {
            model: MODELS[0],
            effort: EFFORTS[1],
            mode: MODES[0],
        }
    }

    fn options(&self) -> Value {
        json!([
            {"id":"model","name":"Model","category":"model","type":"select","currentValue":self.model,
             "options":[{"value":MODELS[0],"name":"Synthetic replay"},{"value":MODELS[1],"name":"Synthetic replay deep"}]},
            {"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":self.effort,
             "options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]}
        ])
    }

    fn session(&self) -> Value {
        json!({"sessionId":SESSION,"modes":{"currentModeId":self.mode,"availableModes":[
            {"id":"default","name":"Default"},{"id":"acceptEdits","name":"Accept edits"},{"id":"plan","name":"Plan"}
        ]},"configOptions":self.options()})
    }

    /// The frames that answer one `set_config_option` or `set_mode`.
    fn answer(&mut self, id: u64, request: &Value) -> Vec<Value> {
        let params = &request["params"];
        let pick = |choices: &[&'static str], value: &Value| {
            choices
                .iter()
                .copied()
                .find(|choice| Some(*choice) == value.as_str())
        };
        let refused =
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Invalid value"}});
        if request["method"] == "session/set_mode" {
            let Some(mode) = pick(&MODES, &params["modeId"]) else {
                return vec![refused];
            };
            self.mode = mode;
            return vec![
                json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":SESSION,"update":{"sessionUpdate":"current_mode_update","currentModeId":mode}}}),
                json!({"jsonrpc":"2.0","id":id,"result":{}}),
            ];
        }
        match params["configId"].as_str() {
            Some("model") => match pick(&MODELS, &params["value"]) {
                Some(model) => self.model = model,
                None => return vec![refused],
            },
            Some("effort") => match pick(&EFFORTS, &params["value"]) {
                Some(effort) => self.effort = effort,
                None => return vec![refused],
            },
            _ => return vec![refused],
        }
        vec![json!({"jsonrpc":"2.0","id":id,"result":{"configOptions":self.options()}})]
    }
}

/// Says how the client answered the elicitation: its action and, for an
/// accepted form, the field names it filled. Answer values are never echoed.
fn elicitation_echo(response: &Value) -> Result<String, FixtureError> {
    if response.get("id") != Some(&json!(ELICITATION_ID)) || response.get("method").is_some() {
        return Err(FixtureError::InvalidArguments);
    }
    let result = &response["result"];
    let fields = result
        .get("content")
        .and_then(Value::as_object)
        .map(|content| content.keys().take(16).cloned().collect::<Vec<_>>());
    Ok(match (result["action"].as_str(), fields) {
        (Some("accept"), Some(fields)) => format!(
            "Synthetic replay: the form came back with {}.",
            fields.join(", ")
        ),
        (Some("accept"), None) => "Synthetic replay: the link was opened.".to_owned(),
        (Some("decline"), None) => "Synthetic replay: the request was declined.".to_owned(),
        (Some("cancel"), None) => "Synthetic replay: the request was cancelled.".to_owned(),
        _ => return Err(FixtureError::InvalidArguments),
    })
}

fn config_options() -> Value {
    json!([{ "id":"model", "name":"Model", "category":"model", "type":"select", "currentValue":"fixture-model", "options":[{"value":"fixture-model","name":"Synthetic replay"}] }])
}

pub(super) fn emit(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let shape = step["shape"]
        .as_str()
        .ok_or(FixtureError::InvalidArguments)?;
    let frame = match shape {
        "acpUpdate" => {
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":SESSION,"update":substitute(step["update"].clone(), state, 0)?}})
        }
        "acpPermission" => {
            let mut frame = json!({"jsonrpc":"2.0","id":"fixture-permission-1","method":"session/request_permission","params":{"sessionId":SESSION,"toolCall":substitute(step["toolCall"].clone(), state, 0)?,"options":[{"kind":"allow_once","optionId":"fixture-allow-once","name":"Allow once"},{"kind":"allow_always","optionId":"fixture-allow-always","name":"Allow always"},{"kind":"reject_once","optionId":"fixture-reject","name":"Decline"}]}});
            if let Some(meta) = step.get("_meta") {
                frame["params"]["_meta"] = substitute(meta.clone(), state, 0)?;
            }
            frame
        }
        "acpElicitation" => {
            let mut params = substitute(step["params"].clone(), state, 0)?;
            params["sessionId"] = json!(SESSION);
            json!({"jsonrpc":"2.0","id":ELICITATION_ID,"method":"elicitation/create","params":params})
        }
        "acpElicitationEcho" | "acpResourceEcho" => {
            let echo = if shape == "acpResourceEcho" {
                state
                    .embedded_context
                    .as_mut()
                    .and_then(EmbeddedContext::take_echo)
            } else {
                state.user_input_echo.take()
            };
            let text = echo.ok_or(FixtureError::InvalidArguments)?;
            json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":SESSION,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":text}}}})
        }
        "acpAuthRequired" => {
            let id = state
                .last_request_id
                .take()
                .ok_or(FixtureError::InvalidArguments)?;
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"Authentication required"}})
        }
        "acpCompleted" | "acpCancelled" | "acpError" => {
            let id = state
                .prompt_request_id
                .take()
                .ok_or(FixtureError::InvalidArguments)?;
            if state.last_request_id == Some(id) {
                state.last_request_id = None;
            }
            if shape == "acpError" {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32603,"message":"Synthetic replay failure; retry is safe."}})
            } else {
                json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":if shape == "acpCancelled" {"cancelled"} else {"end_turn"}}})
            }
        }
        "acpInitialize"
        | "acpInitializeFork"
        | "acpInitializeConfig"
        | "acpSession"
        | "acpForkResult"
        | "acpAck" => {
            let Some(id) = state.last_request_id.take() else {
                return if shape == "acpAck" {
                    Ok(())
                } else {
                    Err(FixtureError::InvalidArguments)
                };
            };
            let result = match shape {
                "acpInitialize" => {
                    let auth = step.get("authMethods").cloned().unwrap_or(json!([]));
                    // A journey may advertise its own prompt capabilities;
                    // with embedded context it reads what each prompt carries.
                    let prompt = step
                        .get("promptCapabilities")
                        .cloned()
                        .unwrap_or(json!({"image":true}));
                    if !prompt.is_object() {
                        return Err(FixtureError::InvalidArguments);
                    }
                    if prompt["embeddedContext"] == true {
                        state.embedded_context = Some(EmbeddedContext::default());
                    }
                    json!({"protocolVersion":1,"agentCapabilities":{"loadSession":false,"promptCapabilities":prompt,"sessionCapabilities":{"close":{}}},"models":[{"id":"fixture-model","name":"Synthetic replay"}],"authMethods":auth})
                }
                // Unstable `session/fork`, advertised as claude-agent-acp 0.76.0 does.
                "acpInitializeFork" => {
                    json!({"protocolVersion":1,"agentCapabilities":{"loadSession":false,"promptCapabilities":{"image":true},"sessionCapabilities":{"close":{},"fork":{}}},"models":[{"id":"fixture-model","name":"Synthetic replay"}],"authMethods":[]})
                }
                // Resumable, with a model and effort to choose.
                "acpInitializeConfig" => {
                    state.acp_config = Some(AcpConfig::defaults());
                    json!({"protocolVersion":1,"agentCapabilities":{"loadSession":false,"promptCapabilities":{"image":true},"sessionCapabilities":{"close":{},"resume":{}}},"models":[{"id":MODELS[0],"name":"Synthetic replay"},{"id":MODELS[1],"name":"Synthetic replay deep"}],"authMethods":[]})
                }
                "acpForkResult" => json!({"sessionId": codex::FORK_THREAD_ID}),
                "acpSession" if state.acp_config.is_some() => {
                    let config = AcpConfig::defaults();
                    let session = config.session();
                    state.acp_config = Some(config);
                    session
                }
                "acpSession" => {
                    json!({"sessionId":SESSION,"modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"},{"id":"plan","name":"Plan"}]},"configOptions":config_options()})
                }
                _ => json!({}),
            };
            json!({"jsonrpc":"2.0","id":id,"result":result})
        }
        _ => return Err(FixtureError::InvalidArguments),
    };
    codex::emit_json(output, state, &frame)
}

pub(super) fn substitute(
    value: Value,
    state: &WireState,
    depth: usize,
) -> Result<Value, FixtureError> {
    if depth > 32 {
        return Err(FixtureError::ExpandedLimit);
    }
    Ok(match value {
        Value::String(text) => {
            let root = std::env::current_dir().map_err(|_| FixtureError::Io)?;
            let text = substitute_root(&text, &root)?
                .replace("$SESSION", SESSION)
                .replace(
                    "$TURN",
                    state.current_turn_id.as_deref().unwrap_or("fixture-turn"),
                );
            // What the client sent goes in last, so its text is never read
            // as a placeholder.
            let text = quote(
                text,
                "$CODEX_TURN_CONFIG",
                state.codex_turn_config.as_deref(),
            )?;
            Value::String(quote(text, "$PROMPT_ECHO", state.prompt_echo.as_deref())?)
        }
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| substitute(value, state, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| Ok((key, substitute(value, state, depth + 1)?)))
                .collect::<Result<_, FixtureError>>()?,
        ),
        other => other,
    })
}

/// `text` with `token` replaced by what the client sent; a frame that
/// quotes something the client has not sent fails the case.
fn quote(text: String, token: &str, value: Option<&str>) -> Result<String, FixtureError> {
    if !text.contains(token) {
        return Ok(text);
    }
    Ok(text.replace(token, value.ok_or(FixtureError::InvalidArguments)?))
}

pub(super) fn substitute_root(text: &str, root: &std::path::Path) -> Result<String, FixtureError> {
    if let Some(suffix) = text.strip_prefix("$ROOT/") {
        let mut target = root.to_path_buf();
        for component in suffix.split('/') {
            target.push(component);
        }
        return target
            .to_str()
            .map(str::to_owned)
            .ok_or(FixtureError::InvalidArguments);
    }
    Ok(text.replace(
        "$ROOT",
        root.to_str().ok_or(FixtureError::InvalidArguments)?,
    ))
}
