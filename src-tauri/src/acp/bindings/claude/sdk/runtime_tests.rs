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
        sent: Vec::new(),
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

#[test]
fn a_prompt_claude_code_never_received_frees_the_next_prompt() {
    let mut runtime = ready();
    let prompt = |text: &str| AdapterCommand::Prompt {
        prompt: Prompt::new("prompt", text).unwrap(),
        client_message_id: RequiredText::new("client", text).unwrap(),
        access_mode: crate::settings_store::SessionAccessMode::Ask,
    };
    let egress = runtime.command(prompt("first")).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::QueueRejected)
        .unwrap();
    assert!(runtime.sent.is_empty());
    let user = send(&mut runtime, prompt("second"));
    assert_eq!(user["message"]["content"][0]["text"], "second");
}

#[test]
fn an_edit_result_with_a_long_patch_finishes_its_row_instead_of_failing_the_session() {
    let mut runtime = ready();
    send(
        &mut runtime,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Rewrite it").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::Ask,
        },
    );
    feed(
        &mut runtime,
        json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Write","input":{"file_path":"/w/a.rs","content":"new"}}]}}),
    );
    let lines: Vec<String> = (0..600).map(|line| format!("-old {line}")).collect();
    let result = frame(
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"The file /w/a.rs has been updated."}]},
        "tool_use_result":{"type":"update","structuredPatch":[{"oldStart":1,"oldLines":600,"newStart":1,"newLines":1,"lines":lines}]}}),
    );
    assert!(crate::acp::transport::validate_structure(&result).is_err());
    let (events, _) = runtime
        .discard_invalid_response(&result)
        .unwrap()
        .into_parts();
    assert!(
        !matches!(events[..], [AdapterEvent::Absorbed]),
        "{events:?}"
    );

    let tools: Vec<String> = (0..600).map(|tool| format!("tool-{tool}")).collect();
    let request = frame(
        json!({"type":"control_request","request_id":"r9","request":{"subtype":"can_use_tool","tool_name":"mcp__x__y","input":{"items":tools}}}),
    );
    assert!(crate::acp::transport::validate_structure(&request).is_err());
    let (_, egress) = runtime
        .discard_invalid_response(&request)
        .unwrap()
        .into_parts();
    let reply: Value = serde_json::from_slice(egress[0].frame().payload()).unwrap();
    assert_eq!(reply["response"]["subtype"], "error");
}

fn prompted() -> ClaudeSdkRuntime {
    let mut runtime = ready();
    send(
        &mut runtime,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Fetch them all").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::Ask,
        },
    );
    runtime
}

#[test]
fn subagent_and_parallel_approvals_past_the_card_limit_keep_the_session() {
    let mut runtime = prompted();
    feed(
        &mut runtime,
        json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"task","name":"Task","input":{"prompt":"look"}}]}}),
    );
    let mut cards = Vec::new();
    for n in 0..10 {
        let (events, egress) = runtime
            .ingest(
                &frame(json!({"type":"control_request","request_id":format!("cli-{n}"),"request":{
                    "subtype":"can_use_tool","tool_name":"WebFetch","tool_use_id":format!("sub-{n}"),"input":{"url":"https://example.com"}}})),
                &Mapper,
            )
            .unwrap()
            .into_parts();
        match events.iter().find_map(|event| match event {
            AdapterEvent::PermissionRequest {
                external_request_id,
                ..
            } => Some(external_request_id.clone()),
            _ => None,
        }) {
            Some(card) => cards.push(card),
            None => {
                assert!(n >= 8, "approval {n} has a card");
                let denial: Value = serde_json::from_slice(egress[0].frame().payload()).unwrap();
                assert_eq!(denial["response"]["response"]["behavior"], "deny");
            }
        }
    }
    assert_eq!(cards.len(), 8);
    let finished = feed(
        &mut runtime,
        json!({"type":"user","parent_tool_use_id":"task","message":{"content":[{"type":"tool_result","tool_use_id":"sub-0","content":"page"}]}}),
    );
    assert!(
        !matches!(finished[..], [AdapterEvent::Absorbed]),
        "{finished:?}"
    );
    let answer = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(cards[1].as_str()).unwrap(),
            decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
        })
        .unwrap();
    let token = answer.into_token();
    for resolution in [EgressResolution::Queued, EgressResolution::Written] {
        runtime.resolve_egress(token.clone(), resolution).unwrap();
    }
    let events = feed(
        &mut runtime,
        json!({"type":"control_request","request_id":"cli-10","request":{
            "subtype":"can_use_tool","tool_name":"WebFetch","tool_use_id":"sub-10","input":{"url":"https://example.com"}}}),
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AdapterEvent::PermissionRequest { .. }))
    );
}

#[test]
fn a_turn_end_too_large_to_read_still_ends_the_turn() {
    let mut runtime = prompted();
    let denials: Vec<Value> = (0..600)
        .map(|n| json!({"tool_name": format!("t{n}")}))
        .collect();
    let result = frame(
        json!({"type":"result","subtype":"success","is_error":false,"usage":{"input_tokens":1,"output_tokens":2},"permission_denials":denials}),
    );
    assert!(crate::acp::transport::validate_structure(&result).is_err());
    let (events, _) = runtime
        .discard_invalid_response(&result)
        .unwrap()
        .into_parts();
    assert!(matches!(
        events.last(),
        Some(AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        })
    ));
}

#[test]
fn a_turn_ends_cleanly_after_claude_code_withdraws_an_approval() {
    let mut runtime = prompted();
    feed(
        &mut runtime,
        json!({"type":"control_request","request_id":"cli-1","request":{
            "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"t1","input":{"command":"ls"}}}),
    );
    let withdrawn = feed(
        &mut runtime,
        json!({"type":"control_cancel_request","request_id":"cli-1"}),
    );
    assert!(withdrawn.iter().any(|event| matches!(
        event,
        AdapterEvent::PermissionResolved {
            state: crate::acp::lifecycle::ServerRequestState::Cancelled,
            ..
        }
    )));
    let ended = feed(
        &mut runtime,
        json!({"type":"result","subtype":"success","is_error":false}),
    );
    assert!(matches!(
        ended.last(),
        Some(AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        })
    ));
}

#[test]
fn a_turn_that_ends_with_an_approval_still_open_cancels_its_card_first() {
    let mut runtime = prompted();
    feed(
        &mut runtime,
        json!({"type":"control_request","request_id":"cli-1","request":{
            "subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"t1","input":{"command":"ls"}}}),
    );
    let ended = feed(
        &mut runtime,
        json!({"type":"result","subtype":"success","is_error":false}),
    );
    let resolved = ended
        .iter()
        .position(|event| matches!(event, AdapterEvent::PermissionResolved { .. }))
        .expect("the open card is cancelled");
    assert!(resolved < ended.len() - 1);
    assert!(matches!(
        ended.last(),
        Some(AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        })
    ));
}
