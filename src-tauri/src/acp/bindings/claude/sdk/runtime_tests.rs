//! A whole Claude Code turn over the SDK protocol, through the shared Claude
//! session state machine, as the events the store and UI receive.

use serde_json::{Value, json};

use super::*;
use crate::acp::{
    contract::RequiredText,
    intent::{NormalizedPayload, Prompt, SafeResourceRef, UnavailableResourceReason},
    lifecycle::TurnState,
    transport::FrameReader,
};

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        absolute_path
            .strip_prefix("/w/")
            .map(|relative| {
                SafeResourceRef::new(
                    crate::acp::contract::SageRootId::parse("root-claude-sdk").unwrap(),
                    relative,
                )
                .unwrap()
            })
            .ok_or(UnavailableResourceReason::Foreign)
    }
}

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(FrameReader::new(bytes.as_slice()).read_frame())
        .unwrap()
        .unwrap()
}

fn runtime() -> ClaudeSdkRuntime {
    let mut runtime = ClaudeSdkRuntime {
        session: ClaudeRuntime::for_session(
            [7; 32],
            RuntimeEpoch::new(1).unwrap(),
            SageAgentSessionId::parse("session-claude-sdk").unwrap(),
        ),
        bridge: None,
        launch: None,
        binding: Arc::new(AdmittedClaudeSdkBinding::for_test()),
        root: None,
        epoch: RuntimeEpoch::new(1).unwrap(),
    };
    runtime.bind_activation(&ActivationMode::New {
        native_root: "/w".into(),
        model: None,
    });
    runtime
}

/// Sends a command, marks it written, and returns the SDK frame.
fn send(runtime: &mut ClaudeSdkRuntime, command: AdapterCommand) -> Value {
    let egress = runtime.command(command).unwrap();
    let sdk = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    sdk
}

fn reply(runtime: &mut ClaudeSdkRuntime, request: &Value, response: Value) -> Vec<AdapterEvent> {
    feed(
        runtime,
        json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":response}}),
    )
}

fn feed(runtime: &mut ClaudeSdkRuntime, sdk: Value) -> Vec<AdapterEvent> {
    let (events, _) = runtime.ingest(&frame(sdk), &Mapper).unwrap().into_parts();
    events
}

fn ready() -> ClaudeSdkRuntime {
    let mut runtime = runtime();
    let initialize = send(&mut runtime, AdapterCommand::Initialize);
    let events = reply(
        &mut runtime,
        &initialize,
        json!({"commands":[{"name":"review","description":"Review changes"}],"current_permission_mode":"default","models":[{"value":"default","displayName":"Default"}]}),
    );
    assert!(matches!(events[..], [AdapterEvent::Initialized { .. }]));
    let capabilities = runtime.capabilities().unwrap();
    assert!(capabilities.resume_session && capabilities.prompt_resources);
    let session = send(
        &mut runtime,
        AdapterCommand::NewSession {
            native_root: "/w".into(),
            model: None,
        },
    );
    let events = reply(&mut runtime, &session, json!({"applied":{}}));
    let AdapterEvent::SessionReady { id, loaded: false } = &events[0] else {
        panic!("session ready first: {events:?}");
    };
    assert_eq!(
        Some(id.external_id.as_str()),
        runtime
            .launch
            .as_ref()
            .map(|launch| launch.session_id.as_str())
    );
    assert_eq!(
        runtime.available_commands().snapshot().commands[0].name,
        "review"
    );
    assert!(runtime.session_config().is_some());
    runtime
}

#[test]
fn a_claude_code_turn_reaches_the_store_as_shared_events() {
    let mut runtime = ready();
    let user = send(
        &mut runtime,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Fix the bug").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::Ask,
        },
    );
    assert_eq!(user["type"], "user");
    assert_eq!(user["message"]["content"][0]["text"], "Fix the bug");

    let mut events = Vec::new();
    for sdk in [
        json!({"type":"system","subtype":"init","session_id":"x"}),
        json!({"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_start","message":{"id":"m1"}}}),
        json!({"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"Looking"}}}),
        json!({"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Editing lib.rs"}}}),
        json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m1","content":[
            {"type":"text","text":"Editing lib.rs"},
            {"type":"tool_use","id":"toolu_1","name":"Edit","input":{"file_path":"/w/src/lib.rs","old_string":"a","new_string":"b"}}]}}),
        json!({"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]}}),
        json!({"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2}}),
    ] {
        events.extend(feed(&mut runtime, sdk));
    }
    let payloads: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Item { payload, .. } => Some(payload),
            _ => None,
        })
        .collect();
    assert!(payloads.iter().any(|p| matches!(p, NormalizedPayload::ThoughtSummary { summary } if summary.as_str() == "Looking")));
    assert!(payloads.iter().any(
        |p| matches!(p, NormalizedPayload::Text { text } if text.as_str() == "Editing lib.rs")
    ));
    assert!(
        payloads
            .iter()
            .any(|p| matches!(p, NormalizedPayload::ToolCall { .. }))
    );
    assert!(events.iter().any(|event| matches!(event, AdapterEvent::FilePair { old_text, new_text, .. } if old_text == "a" && new_text == "b")));
    assert!(matches!(
        events.last(),
        Some(AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        })
    ));
}

#[test]
fn a_tool_approval_round_trips_to_claude_code() {
    let mut runtime = ready();
    send(
        &mut runtime,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Run tests").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::Ask,
        },
    );
    feed(
        &mut runtime,
        json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m","content":[
        {"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}),
    );
    let events = feed(
        &mut runtime,
        json!({"type":"control_request","request_id":"cli-1","request":{
        "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"t1","input":{"command":"cargo test"}}}),
    );
    let request_id = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::PermissionRequest {
                external_request_id,
                ..
            } => Some(external_request_id.clone()),
            _ => None,
        })
        .expect("permission request");
    let egress = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(request_id.as_str()).unwrap(),
            decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
        })
        .unwrap();
    let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(
        sdk["response"],
        json!({"subtype":"success","request_id":"cli-1","response":{"behavior":"allow","updatedInput":{"command":"cargo test"}}})
    );
}

#[test]
fn a_new_launch_names_its_session_and_a_resume_names_the_stored_one() {
    let runtime = runtime();
    let launch = runtime.launch.as_ref().unwrap();
    assert!(!launch.resume);
    assert!(uuid::Uuid::parse_str(&launch.session_id).is_ok());
    let mut resumed = super::tests::runtime();
    resumed.bind_activation(&ActivationMode::Resume {
        external_session_id: crate::acp::intent::ExternalId::new("externalSessionId", "s-9")
            .unwrap(),
        native_root: "/w".into(),
        model: Some(crate::acp::intent::ModelId::new("modelId", "opus").unwrap()),
    });
    let launch = resumed.launch.as_ref().unwrap();
    assert_eq!(
        (
            launch.session_id.as_str(),
            launch.resume,
            launch.model.as_deref()
        ),
        ("s-9", true, Some("opus"))
    );
}
