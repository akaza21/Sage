use serde_json::{Value, json};

use super::*;
use crate::acp::{transcript_quote::Prompt, transport::FrameReader};
use sage_acp_protocol::{
    contract::RequiredText,
    intent::{NormalizedPayload, SafeResourceRef, UnavailableResourceReason},
    lifecycle::TurnState,
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
                    sage_acp_protocol::contract::SageRootId::parse("root-claude-sdk").unwrap(),
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
        children: Default::default(),
        native: None,
        usage: Arc::default(),
        sent: Vec::new(),
        steering: Default::default(),
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
        json!({"commands":[{"name":"review","description":"Review changes"},
            {"name":"status","description":"A user skill"},
            {"name":"mcp","description":"Manage MCP servers","argumentHint":"[reconnect]","builtin":true,"aliases":["servers"]}],
            "current_permission_mode":"default","models":[{"value":"default","displayName":"Default"}]}),
    );
    let [AdapterEvent::Initialized { capabilities: told }] = &events[..] else {
        panic!("initialized: {events:?}");
    };
    let capabilities = runtime.capabilities().unwrap();
    assert_eq!(told, &capabilities);
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
    let listed = runtime.available_commands().snapshot().commands;
    let names: Vec<_> = listed.iter().map(|command| command.name.as_str()).collect();
    assert_eq!(names, ["status", "mcp", "review"]);
    let mcp = &listed[1];
    assert_eq!(
        (mcp.description.as_str(), mcp.input_hint.as_deref()),
        ("Manage MCP servers", None)
    );
    assert_eq!(mcp.aliases, ["servers"]);
    assert_eq!(
        listed[0].description,
        "Show the session's model, access, context and usage"
    );
    assert_eq!(
        mcp.dispatch,
        crate::acp::slash_commands::CommandDispatch::Panel(
            crate::acp::slash_commands::CommandPanel::Mcp
        )
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
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
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
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
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
            decision: sage_acp_protocol::permissions::PermissionDecision::AllowOnce,
        })
        .unwrap();
    let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(
        sdk["response"],
        json!({"subtype":"success","request_id":"cli-1","response":{"behavior":"allow","updatedInput":{"command":"cargo test"}}})
    );
}

#[test]
fn always_allowing_an_edit_keeps_the_turn_running_in_accept_edits() {
    let mut runtime = ready();
    send(
        &mut runtime,
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Write notes").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
        },
    );
    let input = json!({"file_path":"/w/notes.txt","content":"x\n"});
    feed(
        &mut runtime,
        json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m","content":[
        {"type":"tool_use","id":"w1","name":"Write","input":input}]}}),
    );
    let events = feed(
        &mut runtime,
        json!({"type":"control_request","request_id":"cli-w","request":{
        "subtype":"can_use_tool","tool_name":"Write","tool_use_id":"w1","input":input,
        "permission_suggestions":[
            {"type":"setMode","mode":"acceptEdits","destination":"session"},
            {"type":"setMode","mode":"bypassPermissions","destination":"session"}]}}),
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
        .expect("edit card");
    let egress = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(request_id.as_str()).unwrap(),
            decision: sage_acp_protocol::permissions::PermissionDecision::AllowAlways,
        })
        .unwrap();
    let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(
        sdk["response"]["response"]["updatedPermissions"],
        json!([{"type":"setMode","mode":"acceptEdits","destination":"session"}])
    );
    let status =
        json!({"type":"system","subtype":"status","status":null,"permissionMode":"acceptEdits"});
    let (events, egress) = runtime
        .ingest(&frame(status), &Mapper)
        .unwrap()
        .into_parts();
    assert!(egress.is_empty(), "the turn was cancelled");
    assert!(!format!("{events:?}").contains("Notice"), "{events:?}");
}

#[test]
fn approving_a_plan_leaves_plan_mode_for_the_mode_the_user_picked() {
    use sage_acp_protocol::permissions::PermissionDecision;
    for (decision, mode) in [
        (PermissionDecision::AllowOnce, "default"),
        (PermissionDecision::AllowAlways, "acceptEdits"),
        (PermissionDecision::Deny, "plan"),
    ] {
        let mut runtime = ready();
        send(
            &mut runtime,
            AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Plan the fix").unwrap(),
                client_message_id: RequiredText::new("client", "client-1").unwrap(),
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
            },
        );
        let input = json!({"plan":"1. Fix the bug"});
        feed(
            &mut runtime,
            json!({"type":"assistant","parent_tool_use_id":null,"message":{"id":"m","content":[
            {"type":"tool_use","id":"p1","name":"ExitPlanMode","input":input}]}}),
        );
        let events = feed(
            &mut runtime,
            json!({"type":"control_request","request_id":"cli-p","request":{
            "subtype":"can_use_tool","tool_name":"ExitPlanMode","tool_use_id":"p1","input":input}}),
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
            .expect("plan card");
        let egress = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: SageServerRequestId::parse(request_id.as_str()).unwrap(),
                decision,
            })
            .unwrap();
        let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
        let expected = if mode == "plan" {
            json!({"behavior":"deny","message":"The user chose to keep planning.","interrupt":true})
        } else {
            json!({"behavior":"allow","updatedInput":input,
                "updatedPermissions":[{"type":"setMode","mode":mode,"destination":"session"}]})
        };
        assert_eq!(sdk["response"]["response"], expected);
    }
}

#[test]
fn a_new_launch_names_its_session_and_a_resume_names_the_stored_one() {
    let runtime = runtime();
    let launch = runtime.launch.as_ref().unwrap();
    assert!(!launch.resume);
    assert!(uuid::Uuid::parse_str(&launch.session_id).is_ok());
    let mut resumed = super::tests::runtime();
    resumed.bind_activation(&ActivationMode::Resume {
        external_session_id: sage_acp_protocol::intent::ExternalId::new("externalSessionId", "s-9")
            .unwrap(),
        native_root: "/w".into(),
        model: Some(sage_acp_protocol::intent::ModelId::new("modelId", "opus").unwrap()),
        rewind_at: None,
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
        access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
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
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
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
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
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
            decision: sage_acp_protocol::permissions::PermissionDecision::AllowOnce,
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
            state: sage_acp_protocol::lifecycle::ServerRequestState::Cancelled,
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

#[test]
fn a_compaction_reaches_the_store_as_a_session_activity() {
    let mut runtime = prompted();
    let events = feed(
        &mut runtime,
        json!({"type":"system","subtype":"status","status":"compacting"}),
    );
    assert!(matches!(
        events[..],
        [AdapterEvent::Session {
            event: sage_acp_protocol::intent::session_event::SessionScopedEvent::Activity(_),
            ..
        }]
    ));
}

#[test]
fn claudes_questions_become_a_question_card_and_the_answers_return_as_its_input() {
    use sage_acp_protocol::intent::user_input::UserInputAnswers;
    let mut runtime = prompted();
    feed(
        &mut runtime,
        json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"ask1","name":"AskUserQuestion","input":{}}]}}),
    );
    let input = json!({"questions":[
        {"question":"Which test runner?","header":"Runner","multiSelect":false,
         "options":[{"label":"Vitest","description":"Fast"},{"label":"Jest","description":"Familiar"}]},
        {"question":"Which checks?","header":"Checks","multiSelect":true,
         "options":[{"label":"Lint","description":""},{"label":"Tests","description":""}]}]});
    let events = feed(
        &mut runtime,
        json!({"type":"control_request","request_id":"cli-q","request":{
            "subtype":"can_use_tool","tool_name":"AskUserQuestion","tool_use_id":"ask1","input":input}}),
    );
    let (request_id, questions) = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::PermissionRequest {
                external_request_id,
                payload: NormalizedPayload::PermissionRequest { user_input, .. },
                ..
            } => Some((external_request_id.clone(), user_input.clone())),
            _ => None,
        })
        .expect("question card");
    let questions = questions.expect("the card asks questions");
    assert_eq!(
        questions.questions.as_slice()[0].question.as_str(),
        "Which test runner?"
    );
    assert!(questions.questions.as_slice()[1].multi_select);
    let answers: UserInputAnswers = serde_json::from_value(json!([
        {"questionId":"q0","selectedOption":"Vitest"},
        {"questionId":"q1","selectedOptions":["Lint","Tests"],"text":"Types"}]))
    .unwrap();
    let egress = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(request_id.as_str()).unwrap(),
            decision: sage_acp_protocol::permissions::PermissionDecision::Answer(answers),
        })
        .unwrap();
    let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    let response = &sdk["response"]["response"];
    assert_eq!(response["behavior"], "allow");
    assert_eq!(
        response["updatedInput"]["answers"],
        json!({"Which test runner?":"Vitest","Which checks?":"Lint, Tests, Types"})
    );
    assert_eq!(response["updatedInput"]["questions"], input["questions"]);
}

#[test]
fn a_subagent_fills_its_own_transcript_under_a_collaboration_row() {
    let mut runtime = prompted();
    let spawned = feed(
        &mut runtime,
        json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"toolu_task","name":"Agent",
            "input":{"description":"Find the tests","prompt":"List the test files","subagent_type":"Explore"}}]}}),
    );
    let collaboration = |events: &[AdapterEvent]| {
        events.iter().find_map(|event| match event {
            AdapterEvent::ChildThreadItem {
                payload:
                    NormalizedPayload::Collaboration {
                        agent_states,
                        receiver_labels,
                        ..
                    },
                child_thread_id,
                ..
            } => Some((
                child_thread_id.as_str().to_owned(),
                receiver_labels.as_slice()[0].as_str().to_owned(),
                agent_states.as_slice()[0].1.as_str().to_owned(),
            )),
            _ => None,
        })
    };
    assert_eq!(
        collaboration(&spawned),
        Some((
            "toolu_task".into(),
            "Find the tests".into(),
            "running".into()
        ))
    );
    assert!(spawned.iter().any(|event| matches!(event,
        AdapterEvent::ChildLiveItem { item_id, role: Some(sage_acp_protocol::codex_protocol::ChildTextRole::User), .. }
            if item_id.as_str() == "prompt")));
    let inner = feed(
        &mut runtime,
        json!({"type":"assistant","parent_tool_use_id":"toolu_task","message":{"id":"s1","content":[
            {"type":"text","text":"Looking"},
            {"type":"tool_use","id":"sub-1","name":"Glob","input":{"pattern":"**/*.test.ts"}}]}}),
    );
    assert_eq!(
        inner
            .iter()
            .filter(|event| matches!(event, AdapterEvent::ChildLiveItem { .. }))
            .count(),
        2,
        "{inner:?}"
    );
    let result = feed(
        &mut runtime,
        json!({"type":"user","parent_tool_use_id":"toolu_task","message":{"content":[{"type":"tool_result","tool_use_id":"sub-1","content":"a.test.ts"}]}}),
    );
    assert!(result.iter().any(|event| matches!(event,
        AdapterEvent::ChildLiveItem { item_id, lifecycle, .. }
            if item_id.as_str() == "sub-1" && lifecycle.is_terminal())));
    let ended = feed(
        &mut runtime,
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_task","content":"a.test.ts"}]}}),
    );
    assert_eq!(
        collaboration(&ended),
        Some((
            "toolu_task".into(),
            "Find the tests".into(),
            "completed".into()
        ))
    );
    assert!(ended.iter().any(|event| matches!(
        event,
        AdapterEvent::ChildTurnStatus {
            status: sage_acp_protocol::lifecycle::TurnState::Completed,
            ..
        }
    )));
    let late = feed(
        &mut runtime,
        json!({"type":"assistant","parent_tool_use_id":"toolu_task","message":{"id":"s2","content":[{"type":"text","text":"late"}]}}),
    );
    assert!(matches!(late[..], [AdapterEvent::Absorbed]), "{late:?}");
}

#[test]
fn a_prompt_larger_than_an_ordinary_frame_is_sent_as_a_prompt_frame() {
    let ordinary = sage_core::limits::LIMITS.wire.frame_bytes as usize;
    let user = json!({"type":"user","message":{"content":"x".repeat(ordinary)}});
    assert!(sdk_frame(&user).is_ok());
    let control = json!({"type":"control_request","request":{"note":"x".repeat(ordinary)}});
    assert!(sdk_frame(&control).is_err());
}

fn steer(runtime: &mut ClaudeSdkRuntime, text: &str) -> (Value, u64) {
    let egress = runtime
        .command(AdapterCommand::Steer {
            prompt: Prompt::new("prompt", text).unwrap(),
            client_message_id: RequiredText::new("client", "client-2").unwrap(),
        })
        .unwrap();
    let request_id = egress.token().request_id().unwrap();
    let sdk = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    (sdk, request_id)
}

fn turn_ended(events: &[AdapterEvent]) -> bool {
    events.iter().any(
        |event| matches!(event, AdapterEvent::Turn { state, .. } if *state != TurnState::Running),
    )
}

#[test]
fn a_message_sent_while_claude_works_is_accepted_and_keeps_the_turn_open() {
    let mut runtime = prompted();
    assert!(runtime.capabilities().unwrap().active_turn_steer);
    let (user, request_id) = steer(&mut runtime, "Only the first page");
    assert_eq!(user["message"]["content"][0]["text"], "Only the first page");
    let uuid = user["uuid"].as_str().unwrap().to_owned();
    let queued = feed(
        &mut runtime,
        json!({"type":"command_lifecycle","command_uuid":uuid,"state":"queued","uuid":"e1"}),
    );
    assert!(matches!(
        queued[..],
        [AdapterEvent::SteerResponse { request_id: id, accepted: true }] if id == request_id
    ));
    let first = feed(
        &mut runtime,
        json!({"type":"result","subtype":"success","is_error":false,"usage":{}}),
    );
    assert!(!turn_ended(&first), "the waiting message runs in this turn");
    let started = feed(
        &mut runtime,
        json!({"type":"command_lifecycle","command_uuid":uuid,"state":"started","uuid":"e2"}),
    );
    assert!(
        !started
            .iter()
            .any(|event| matches!(event, AdapterEvent::SteerResponse { .. })),
        "accepted once"
    );
    let last = feed(
        &mut runtime,
        json!({"type":"result","subtype":"success","is_error":false,"usage":{}}),
    );
    assert!(matches!(
        last.last(),
        Some(AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        })
    ));
}

#[test]
fn a_message_never_written_is_forgotten_and_one_after_the_turn_is_refused() {
    let mut runtime = prompted();
    let egress = runtime
        .command(AdapterCommand::Steer {
            prompt: Prompt::new("prompt", "lost").unwrap(),
            client_message_id: RequiredText::new("client", "client-2").unwrap(),
        })
        .unwrap();
    let request_id = egress.token().request_id().unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::QueueRejected)
        .unwrap();
    let end = feed(
        &mut runtime,
        json!({"type":"result","subtype":"success","is_error":false,"usage":{}}),
    );
    assert!(end.iter().any(|event| matches!(
        event,
        AdapterEvent::SteerResponse { request_id: id, accepted: false } if *id == request_id
    )));
    assert!(turn_ended(&end));
    assert!(matches!(
        runtime.command(AdapterCommand::Steer {
            prompt: Prompt::new("prompt", "late").unwrap(),
            client_message_id: RequiredText::new("client", "client-3").unwrap(),
        }),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn compact_runs_as_claude_codes_own_command_and_ends_its_turn() {
    let mut runtime = ready();
    assert!(runtime.capabilities().unwrap().native_session_compact);
    let mut prompt = Prompt::new("sessionAction", "/compact").unwrap();
    prompt.action = Some(crate::acp::session_action::SessionAction::Compact {});
    let user = send(
        &mut runtime,
        AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
        },
    );
    assert_eq!(user["message"]["content"][0]["text"], "/compact");
    let mut events = Vec::new();
    // As Claude Code 2.1.322 reports a compaction.
    for sdk in [
        json!({"type":"system","subtype":"status","status":"compacting"}),
        json!({"type":"system","subtype":"status","status":null}),
        json!({"type":"system","subtype":"init","session_id":"x"}),
        json!({"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"manual","pre_tokens":900}}),
        json!({"type":"user","parent_tool_use_id":null,"isReplay":false,"message":{"role":"user","content":"This session is being continued from a previous conversation. Summary: secret plans"}}),
        json!({"type":"user","parent_tool_use_id":null,"isReplay":true,"message":{"role":"user","content":"<local-command-stdout>Compacted </local-command-stdout>"}}),
        json!({"type":"result","subtype":"success","is_error":false,"result":"","num_turns":0,"usage":{}}),
    ] {
        events.extend(feed(&mut runtime, sdk));
    }
    assert!(
        !format!("{events:?}").contains("secret plans"),
        "the summary stays Claude Code's own"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AdapterEvent::Session { .. }))
    );
    assert!(matches!(
        events.last(),
        Some(AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        })
    ));
}

#[test]
fn a_session_forks_through_claude_code_and_holds_nothing_to_release() {
    let mut runtime = ready();
    let capabilities = runtime.capabilities().unwrap();
    assert!(capabilities.native_session_fork);
    let egress = runtime.fork_session().unwrap();
    let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(runtime.native_rpc_pending());
    reply(&mut runtime, &sdk, json!({}));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(crate::acp::goal::NativeRpcReply::Rejected)
    );
    let fork = sage_acp_protocol::intent::ExternalId::new("externalSessionId", "f").unwrap();
    assert!(matches!(runtime.release_fork(&fork, true), Ok(None)));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(crate::acp::goal::NativeRpcReply::Released)
    );
    assert!(runtime.defer_fork_release(&fork, true));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(crate::acp::goal::NativeRpcReply::Released)
    );
}

#[test]
fn mcp_servers_list_through_claude_code_and_an_abandoned_list_frees_the_session() {
    use crate::acp::goal::NativeRpcReply;
    use sage_acp_protocol::session_control::SessionControl;
    let mut runtime = ready();
    let list = |runtime: &mut ClaudeSdkRuntime| {
        let egress = runtime
            .session_control(&SessionControl::ReadMcpServers)
            .unwrap();
        let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .unwrap();
        assert_eq!(sdk["request"]["subtype"], "mcp_status");
        sdk
    };
    let first = list(&mut runtime);
    assert!(runtime.native_rpc_pending());
    runtime.abandon_passive_read();
    assert!(!runtime.native_rpc_pending());
    let late = reply(&mut runtime, &first, json!({"mcpServers": []}));
    assert!(matches!(late[..], [AdapterEvent::Absorbed]), "{late:?}");
    assert_eq!(runtime.take_native_rpc_reply(), None);
    let second = list(&mut runtime);
    reply(
        &mut runtime,
        &second,
        json!({"mcpServers": [{"name": "files", "status": "connected", "tools": [{"name": "read"}]}]}),
    );
    let Some(NativeRpcReply::McpServers(inventory)) = runtime.take_native_rpc_reply() else {
        panic!("a server list")
    };
    assert_eq!(inventory.servers[0].tools, 1);
    assert!(inventory.actions.reconnect && !inventory.actions.reload);
}

#[test]
fn usage_limits_read_through_claude_code_and_an_api_key_account_reports_none() {
    use crate::acp::goal::NativeRpcReply;
    use sage_acp_protocol::usage::UsageState;
    let mut runtime = ready();
    let usage = runtime.usage().expect("Claude Code reports usage");
    let read = |runtime: &mut ClaudeSdkRuntime| {
        let egress = runtime.read_usage().unwrap();
        let sdk: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .unwrap();
        assert_eq!(
            sdk["request"],
            json!({"subtype": "get_usage", "skip_behaviors": true})
        );
        sdk
    };
    let first = read(&mut runtime);
    reply(
        &mut runtime,
        &first,
        json!({"subscription_type": "pro", "rate_limits_available": true, "rate_limits": {
            "five_hour": {"utilization": 1, "resets_at": "2026-11-12T17:20:00Z"},
        }}),
    );
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::UsageRead)
    );
    let UsageState::Usage { snapshot } = usage.state() else {
        panic!("a usage snapshot")
    };
    assert_eq!(snapshot.plan.as_deref(), Some("Pro"));
    assert_eq!(snapshot.windows[0].resets_at, Some(1_794_504_000));
    let second = read(&mut runtime);
    reply(
        &mut runtime,
        &second,
        json!({"subscription_type": null, "rate_limits_available": false, "rate_limits": null}),
    );
    assert_eq!(usage.state(), UsageState::Unsupported);
}

#[test]
fn a_command_longer_than_a_tool_label_keeps_the_session_and_clips_its_label() {
    const COMMAND: &str = r#"Get-ChildItem src-tauri\src -Directory | Select -Expand Name; "---"; Get-ChildItem src-tauri\src -Recurse -File | ? { $_.Name -match 'claude' } | % { $_.FullName.Replace((Get-Location).Path,'') + "  " + (Get-Content $_.FullName | Measure-Object -Line).Lines }; "---FEATURES"; Get-ChildItem src\features -Directory | Select -Expand Name; "--- PTY/terminal"; Get-ChildItem src-tauri\src -Recurse -File | ? { $_.FullName -match 'pty|terminal' } | Select -First 10 | % { $_.FullName.Replace((Get-Location).Path,'') }; "--- acp runtimes"; Get-ChildItem src-tauri\src\acp -Directory -Recurse | Select -First 40 | % { $_.FullName.Replace((Get-Location).Path,'') }"#;
    let mut runtime = prompted();
    let events = feed(
        &mut runtime,
        json!({"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"toolu_1","name":"PowerShell","input":{"command":COMMAND,"description":"Survey Rust modules"}}]}}),
    );
    let (label, input_summary) = events
        .iter()
        .find_map(|event| match event {
            AdapterEvent::Item {
                payload:
                    NormalizedPayload::ToolCall {
                        label,
                        input_summary,
                        ..
                    },
                ..
            } => Some((label.as_str(), input_summary.as_ref().map(|s| s.as_str()))),
            _ => None,
        })
        .expect("the command reaches the store as a tool row");
    assert_eq!(label.len(), 512);
    assert!(COMMAND.starts_with(label.trim_end_matches('\u{2026}')));
    assert!(label.ends_with('\u{2026}'));
    assert_eq!(input_summary, Some(COMMAND));
}
