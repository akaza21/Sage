use std::io::{Read, Write};

use serde_json::{Value, json};

use super::super::FixtureError;
use super::claude_transcript::Transcript;
use super::{WireState, acp, codex};

const MAX_FRAMES_PER_STEP: usize = 32;
const SESSION: &str = "fixture-session";

#[derive(Default)]
pub(super) struct SdkWireState {
    follow_up: Option<FollowUp>,
    lifecycle_events: u64,
    transcript: Option<Transcript>,
    /// What that session held before the last prompt (`$TRANSCRIPT_ECHO`).
    transcript_echo: Option<String>,
    /// A follow-up's frame, saved once a run takes it up, as Claude Code
    /// saves a queued message.
    unsaved_follow_up: Option<Value>,
}

impl SdkWireState {
    pub(super) fn launched(harness_args: &[String]) -> Result<Self, FixtureError> {
        if harness_args.is_empty() {
            return Ok(Self::default());
        }
        let config = std::env::var_os("CLAUDE_CONFIG_DIR").ok_or(FixtureError::InvalidArguments)?;
        let cwd = std::env::current_dir().map_err(|_| FixtureError::Io)?;
        Self::opened(harness_args, std::path::Path::new(&config), &cwd)
    }

    pub(super) fn opened(
        harness_args: &[String],
        config: &std::path::Path,
        cwd: &std::path::Path,
    ) -> Result<Self, FixtureError> {
        Ok(Self {
            transcript: Some(Transcript::launched(harness_args, config, cwd)?),
            ..Self::default()
        })
    }

    pub(super) fn transcript_echo(&self) -> Option<&str> {
        self.transcript_echo.as_deref()
    }
}

/// A mid-turn message by the `uuid` the client gave it: waiting until a run
/// takes it up, then running. Only a waiting message can be withdrawn.
enum FollowUp {
    Waiting(String),
    Running(String),
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
    let prompt_text = step.get("promptText").and_then(Value::as_str);
    if (prompt_text.is_some() && kind != "claudeSdkPrompt")
        || step
            .get("relaunch")
            .is_some_and(|relaunch| *relaunch != true || kind != "claudeSdkInitialize")
    {
        return Err(FixtureError::InvalidArguments);
    }
    let expected = match kind {
        "claudeSdkInitialize" => "initialize",
        "claudeSdkSettings" => "get_settings",
        "claudeSdkInterrupt" => "interrupt",
        "claudeSdkPrompt" | "claudeSdkFollowUp" => "user",
        "claudeSdkPermission" => "control_response",
        "claudeSdkIdle" => "idle",
        _ => return Err(FixtureError::InvalidArguments),
    };
    for _ in 0..MAX_FRAMES_PER_STEP {
        let Some(frame) = codex::read_json_line(input, state)? else {
            return if expected == "idle" {
                Ok(())
            } else {
                Err(FixtureError::Io)
            };
        };
        let observed = match frame["type"].as_str() {
            Some("control_request") => answer(&frame, state, output)?,
            Some("user") if frame["message"]["content"].is_array() => "user",
            Some("control_response") if frame["response"]["subtype"] == "success" => {
                "control_response"
            }
            _ => return Err(FixtureError::InvalidArguments),
        };
        if observed == expected {
            if expected == "user" {
                receive(&frame, state, kind == "claudeSdkFollowUp", prompt_text)?;
            }
            return Ok(());
        }
        if observed == "user" || observed == "control_response" {
            return Err(FixtureError::InvalidArguments);
        }
    }
    Err(FixtureError::ExpandedLimit)
}

/// Takes a `user` message. A follow-up must carry the `uuid` Claude Code
/// reports it by, and only one waits at a time; a journey that names the
/// prompt text proves what Sage put on the wire, one text block exactly.
fn receive(
    frame: &Value,
    state: &mut WireState,
    follow_up: bool,
    prompt_text: Option<&str>,
) -> Result<(), FixtureError> {
    let content = &frame["message"]["content"];
    if let Some(text) = prompt_text
        && *content != json!([{"type": "text", "text": text}])
    {
        return Err(FixtureError::InvalidArguments);
    }
    if follow_up {
        let uuid = frame["uuid"]
            .as_str()
            .filter(|uuid| !uuid.is_empty() && uuid.len() <= 64)
            .ok_or(FixtureError::InvalidArguments)?;
        if state.claude_sdk.follow_up.is_some() {
            return Err(FixtureError::InvalidArguments);
        }
        state.claude_sdk.follow_up = Some(FollowUp::Waiting(uuid.to_owned()));
    }
    let blocks = content.as_array().ok_or(FixtureError::InvalidArguments)?;
    state.prompt_echo = Some(super::prompt_echo::blocks(blocks));
    if let Some(transcript) = &mut state.claude_sdk.transcript {
        state.claude_sdk.transcript_echo = Some(transcript.echo());
        if follow_up {
            state.claude_sdk.unsaved_follow_up = Some(frame.clone());
        } else {
            transcript.sent(frame)?;
        }
    }
    Ok(())
}

fn answer(
    frame: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<&'static str, FixtureError> {
    let request = &frame["request"];
    let (subtype, response) = match request["subtype"].as_str() {
        Some("initialize") => ("initialize", initialize()),
        Some("get_settings") => (
            "get_settings",
            json!({"applied": {"model": "claude-fixture", "effort": "medium"}}),
        ),
        Some("set_model") => ("set_model", json!({"model": request["model"]})),
        Some("set_permission_mode") => ("set_permission_mode", json!({"mode": request["mode"]})),
        Some("apply_flag_settings") => ("apply_flag_settings", json!({})),
        Some("interrupt") => ("interrupt", json!({})),
        // Claude Code reports a withdrawn message cancelled before it
        // answers; an id it holds no message for is not cancelled.
        Some("cancel_async_message") => {
            let waiting = matches!(
                &state.claude_sdk.follow_up,
                Some(FollowUp::Waiting(uuid)) if request["message_uuid"].as_str() == Some(uuid)
            );
            if waiting {
                emit_lifecycle("cancelled", state, output)?;
            }
            ("cancel_async_message", json!({"cancelled": waiting}))
        }
        _ => return Err(FixtureError::InvalidArguments),
    };
    let request_id = frame["request_id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 64)
        .ok_or(FixtureError::InvalidArguments)?;
    codex::emit_json(
        output,
        state,
        &json!({"type": "control_response", "response": {
            "subtype": "success", "request_id": request_id, "response": response,
        }}),
    )?;
    Ok(subtype)
}

fn initialize() -> Value {
    let model = |value: &str, name: &str| {
        json!({
            "value": value, "displayName": name, "description": "Synthetic replay",
            "supportsEffort": true, "supportedEffortLevels": ["low", "medium", "high"],
        })
    };
    json!({
        "commands": [{"name": "compact", "description": "Compact the conversation", "argumentHint": "[focus]"}],
        "models": [model("default", "Synthetic replay"), model("fixture-fast", "Synthetic fast")],
        "current_permission_mode": "default",
    })
}

/// Claude Code's `command_lifecycle` frame for the waiting follow-up: it is
/// `queued` on arrival, `started` when a run takes it up, and `completed`
/// or `cancelled` once it no longer waits.
fn emit_lifecycle(
    lifecycle: &str,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    let sdk = &mut state.claude_sdk;
    let uuid = match (lifecycle, sdk.follow_up.take()) {
        ("queued", Some(FollowUp::Waiting(uuid))) => {
            sdk.follow_up = Some(FollowUp::Waiting(uuid.clone()));
            uuid
        }
        ("started", Some(FollowUp::Waiting(uuid))) => {
            sdk.follow_up = Some(FollowUp::Running(uuid.clone()));
            if let (Some(transcript), Some(frame)) =
                (&mut sdk.transcript, sdk.unsaved_follow_up.take())
            {
                transcript.sent(&frame)?;
            }
            uuid
        }
        ("completed", Some(FollowUp::Running(uuid))) => uuid,
        ("cancelled", Some(FollowUp::Waiting(uuid))) => {
            sdk.unsaved_follow_up = None;
            uuid
        }
        _ => return Err(FixtureError::InvalidArguments),
    };
    sdk.lifecycle_events += 1;
    let event = format!("fixture-lifecycle-{}", sdk.lifecycle_events);
    let session = sdk
        .transcript
        .as_ref()
        .map_or(SESSION, Transcript::session)
        .to_owned();
    codex::emit_json(
        output,
        state,
        &json!({"type": "command_lifecycle", "command_uuid": uuid, "state": lifecycle,
            "uuid": event, "session_id": session}),
    )
}

pub(super) fn emit(
    step: &Value,
    state: &mut WireState,
    output: &mut impl Write,
) -> Result<(), FixtureError> {
    if step["shape"] == "claudeSdkLifecycle" {
        let lifecycle = step["state"]
            .as_str()
            .ok_or(FixtureError::InvalidArguments)?;
        if lifecycle == "cancelled" {
            return Err(FixtureError::InvalidArguments);
        }
        return emit_lifecycle(lifecycle, state, output);
    }
    let mut frame = acp::substitute(step["frame"].clone(), state, 0)?;
    if !frame.is_object() || frame["type"].as_str().is_none() {
        return Err(FixtureError::InvalidArguments);
    }
    if let Some(transcript) = &mut state.claude_sdk.transcript {
        transcript.replayed(&mut frame)?;
    }
    codex::emit_json(output, state, &frame)
}
