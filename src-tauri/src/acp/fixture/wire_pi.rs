use std::{
    collections::BTreeSet,
    io::{Read, Write},
};

use serde_json::{Value, json};

use crate::acp::limits::LIMITS;

use super::super::FixtureError;
use super::{WireState, acp, codex};

/// Most commands a replayed Pi lists.
const MAX_COMMANDS: usize = 64;
/// Most text fields one `piNative` step may fill.
const MAX_FILLS: usize = 32;

pub(super) struct PiWireState {
    model: &'static str,
    thinking: String,
    ids: BTreeSet<String>,
    /// What `get_commands` lists: the extensions, prompt templates and
    /// skills this Pi loaded at startup.
    commands: Value,
}

impl PiWireState {
    pub(super) fn new() -> Self {
        Self {
            model: "fixture-model",
            thinking: "medium".into(),
            ids: BTreeSet::new(),
            commands: json!([]),
        }
    }
}

pub(super) fn await_request(
    input: &mut impl Read,
    output: &mut impl Write,
    state: &mut WireState,
    step: &Value,
) -> Result<(), FixtureError> {
    let kind = step["kind"]
        .as_str()
        .ok_or(FixtureError::InvalidArguments)?;
    // A journey names the commands Pi loads as it starts.
    if let Some(commands) = step.get("commands") {
        let listed = commands
            .as_array()
            .filter(|commands| kind == "piInitialize" && commands.len() <= MAX_COMMANDS)
            .ok_or(FixtureError::InvalidArguments)?;
        if !listed.iter().all(|command| command["name"].is_string()) {
            return Err(FixtureError::InvalidArguments);
        }
        state.pi.commands = acp::substitute(commands.clone(), state, 0)?;
    }
    let expected = match kind {
        "piInitialize" => "get_state",
        "piModels" => "get_available_models",
        // A new chat takes the session Pi opened at launch.
        "piNew" => "get_state",
        "piPrompt" => "prompt",
        "piThinkingLevels" | "piThinkingLevelsUnsupported" => "get_available_thinking_levels",
        "piCommandsUnsupported" => "get_commands",
        "piAbort" => "abort",
        "piSteer" => "steer",
        "piClearQueue" => "clear_queue",
        "piDialogCancelled" | "piDialogAnswered" => "extension_ui_response",
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
            let answered = kind == "piDialogAnswered"
                && id == "fixture-question-1"
                && request["value"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty());
            let cancelled = kind == "piDialogCancelled"
                && id == "fixture-dialog-1"
                && request["cancelled"] == true;
            return if expected == command && (answered || cancelled) {
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
                    "prompt"
                        | "abort"
                        | "steer"
                        | "clear_queue"
                        | "idle"
                        | "get_available_thinking_levels"
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
            json!({"model":model(state.pi.model),"thinkingLevel":state.pi.thinking,"isStreaming":false,"sessionId":"fixture-pi-session","sessionFile":"$ROOT/2026-01-01T00-00-00-000Z_fixture-pi-session.jsonl"}),
            state,
            0,
        )?,
        "get_available_models" => {
            json!({"models":[model("fixture-model"),model("fixture-model-two")]})
        }
        "get_available_thinking_levels" => json!({"levels":["off","low","medium","high"]}),
        "get_commands" => json!({"commands":state.pi.commands}),
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
        "prompt" => {
            let message = request["message"]
                .as_str()
                .ok_or(FixtureError::InvalidArguments)?;
            if message.is_empty() {
                return Err(FixtureError::InvalidArguments);
            }
            let mut blocks = vec![json!({"type": "text", "text": message})];
            if let Some(images) = request["images"].as_array() {
                blocks.extend(images.iter().cloned());
            }
            state.prompt_echo = Some(super::prompt_echo::blocks(&blocks));
            state.turn_count = state
                .turn_count
                .checked_add(1)
                .ok_or(FixtureError::ExpandedLimit)?;
            json!({"disposition":"started"})
        }
        "steer"
            if request["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()) =>
        {
            json!({"disposition":"queued"})
        }
        "clear_queue" => json!({"steering":[],"followUp":[]}),
        "abort" => json!({}),
        _ => return Err(FixtureError::InvalidArguments),
    })
}

pub(super) fn emit(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let mut frame = acp::substitute(step["frame"].clone(), state, 0)?;
    if !frame.is_object() || frame["type"].as_str().is_none() || frame["type"] == "response" {
        return Err(FixtureError::InvalidArguments);
    }
    if let Some(fill) = step.get("fill") {
        fill_text(&mut frame, fill)?;
    }
    codex::emit_json(output, state, &frame)
}

/// Fills each text field `fill.pointers` names with `fill.unit` repeated to
/// `fill.bytes`, so a journey can replay output as large as a real run's
/// (a read tool's result, or `agent_end` repeating the whole run) without
/// spelling it out. Each field stays within one frame, and all of them
/// within the case's output budget.
fn fill_text(frame: &mut Value, fill: &Value) -> Result<(), FixtureError> {
    let pointers = fill["pointers"]
        .as_array()
        .filter(|pointers| !pointers.is_empty() && pointers.len() <= MAX_FILLS)
        .ok_or(FixtureError::InvalidArguments)?;
    let unit = fill["unit"]
        .as_str()
        .filter(|unit| !unit.is_empty() && unit.len() <= 256 && unit.is_ascii())
        .ok_or(FixtureError::InvalidArguments)?;
    let bytes = fill["bytes"]
        .as_u64()
        .filter(|bytes| *bytes <= LIMITS.wire.frame_bytes)
        .ok_or(FixtureError::InvalidArguments)?;
    if bytes.saturating_mul(pointers.len() as u64) > LIMITS.fixture.expanded_bytes_per_case {
        return Err(FixtureError::ExpandedLimit);
    }
    let bytes = usize::try_from(bytes).map_err(|_| FixtureError::ExpandedLimit)?;
    let mut text = unit.repeat(bytes.div_ceil(unit.len()));
    text.truncate(bytes);
    for pointer in pointers {
        let field = pointer
            .as_str()
            .and_then(|pointer| frame.pointer_mut(pointer))
            .filter(|field| field.is_string())
            .ok_or(FixtureError::InvalidArguments)?;
        *field = Value::String(text.clone());
    }
    Ok(())
}
