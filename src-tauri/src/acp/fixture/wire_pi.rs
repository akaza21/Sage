use std::{
    collections::BTreeSet,
    io::{Read, Write},
};

use serde_json::{Value, json};

use super::super::FixtureError;
use super::{WireState, acp, codex};

pub(super) struct PiWireState {
    model: &'static str,
    thinking: String,
    ids: BTreeSet<String>,
}

impl PiWireState {
    pub(super) fn new() -> Self {
        Self {
            model: "fixture-model",
            thinking: "medium".into(),
            ids: BTreeSet::new(),
        }
    }
}

pub(super) fn await_request(
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
    kind: &str,
) -> Result<(), FixtureError> {
    let expected = match kind {
        "piInitialize" => "get_state",
        "piModels" => "get_available_models",
        "piNew" => "new_session",
        "piPrompt" => "prompt",
        "piThinkingLevels" | "piThinkingLevelsUnsupported" => "get_available_thinking_levels",
        "piCommandsUnsupported" => "get_commands",
        "piAbort" => "abort",
        "piDialogCancelled" => "extension_ui_response",
        "piIdle" => "idle",
        _ => return Err(FixtureError::InvalidArguments),
    };
    for _ in 0..32 {
        let Some(request) = codex::read_json_line(input, state)? else {
            return if expected == "idle" {
                Ok(())
            } else {
                Err(FixtureError::Io)
            };
        };
        let command = request["type"]
            .as_str()
            .ok_or(FixtureError::InvalidArguments)?;
        let id = request["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 512 && !id.contains('\0'))
            .ok_or(FixtureError::InvalidArguments)?
            .to_owned();
        if command == "extension_ui_response" {
            return if expected == command
                && id == "fixture-dialog-1"
                && request["cancelled"] == true
            {
                Ok(())
            } else {
                Err(FixtureError::InvalidArguments)
            };
        }
        let session_request = matches!(
            command,
            "get_state"
                | "get_available_models"
                | "get_available_thinking_levels"
                | "get_commands"
                | "set_model"
                | "set_thinking_level"
        );
        if command != expected
            && !(session_request
                && matches!(
                    expected,
                    "prompt" | "abort" | "idle" | "get_available_thinking_levels"
                ))
        {
            return Err(FixtureError::InvalidArguments);
        }
        if state.pi.ids.len() >= 32 || !state.pi.ids.insert(id.clone()) {
            return Err(FixtureError::ExpandedLimit);
        }
        if command == expected
            && matches!(
                kind,
                "piThinkingLevelsUnsupported" | "piCommandsUnsupported"
            )
        {
            // Older Pi builds omit the request ID for unknown commands.
            codex::emit_json(
                output,
                state,
                &json!({"type":"response","command":command,"success":false,"error":format!("Unknown command: {command}")}),
            )?;
            return Ok(());
        }
        let data = response_data(command, &request, state)?;
        codex::emit_json(
            output,
            state,
            &json!({"type":"response","id":id,"command":command,"success":true,"data":data}),
        )?;
        if command == expected {
            return Ok(());
        }
    }
    Err(FixtureError::ExpandedLimit)
}

fn model(id: &str) -> Value {
    json!({"provider":"openai","id":id,"name":if id == "fixture-model" { "Synthetic replay" } else { "Synthetic alternative" },"api":"openai-responses","baseUrl":"https://example.invalid","reasoning":true,"input":["text"],"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0},"contextWindow":32768,"maxTokens":4096})
}

fn response_data(
    command: &str,
    request: &Value,
    state: &mut WireState,
) -> Result<Value, FixtureError> {
    Ok(match command {
        "get_state" => acp::substitute(
            json!({"model":model(state.pi.model),"thinkingLevel":state.pi.thinking,"isStreaming":false,"sessionFile":"$ROOT/synthetic-session.jsonl"}),
            state,
            0,
        )?,
        "get_available_models" => {
            json!({"models":[model("fixture-model"),model("fixture-model-two")]})
        }
        "get_available_thinking_levels" => json!({"levels":["off","low","medium","high"]}),
        "get_commands" => json!({"commands":[]}),
        "set_model" => {
            if request["provider"] != "openai" {
                return Err(FixtureError::InvalidArguments);
            }
            state.pi.model = match request["modelId"].as_str() {
                Some("fixture-model") => "fixture-model",
                Some("fixture-model-two") => "fixture-model-two",
                _ => return Err(FixtureError::InvalidArguments),
            };
            model(state.pi.model)
        }
        "set_thinking_level" => {
            let level = request["level"]
                .as_str()
                .filter(|level| ["off", "low", "medium", "high"].contains(level))
                .ok_or(FixtureError::InvalidArguments)?;
            state.pi.thinking = level.into();
            json!({})
        }
        "new_session" => json!({"cancelled":false}),
        "prompt" => {
            let message = request["message"]
                .as_str()
                .ok_or(FixtureError::InvalidArguments)?;
            if message.is_empty() {
                return Err(FixtureError::InvalidArguments);
            }
            state.turn_count = state
                .turn_count
                .checked_add(1)
                .ok_or(FixtureError::ExpandedLimit)?;
            json!({})
        }
        "abort" => json!({}),
        _ => return Err(FixtureError::InvalidArguments),
    })
}

pub(super) fn emit(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let frame = acp::substitute(step["frame"].clone(), state, 0)?;
    if !frame.is_object() || frame["type"].as_str().is_none() || frame["type"] == "response" {
        return Err(FixtureError::InvalidArguments);
    }
    codex::emit_json(output, state, &frame)
}
