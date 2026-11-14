use super::*;
use sage_acp_protocol::intent::NormalizedPayload;

const HIDDEN: &str =
    "command hidden: contains content Sage will not display; review before approving";
const TOO_LONG: &str = "command too long to display";

fn permission_frame(tool: &str, request: &str, command: &str) -> Frame {
    frame(json!({
        "id":request,"method":"session/request_permission","params":{
            "sessionId":"claude-session-1","toolCall":{
                "toolCallId":tool,"title":"bash","kind":"execute",
                "rawInput":{"command":command}
            },
            "options":[{"optionId":"allow","kind":"allow_once","name":"Allow"},
                {"optionId":"reject","kind":"reject_once","name":"Reject"}]
        }
    }))
}

fn started(runtime: &mut ClaudeRuntime, tool: &str) {
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":tool,
                    "kind":"execute","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .expect("tool start");
}

fn opened(runtime: &mut ClaudeRuntime) {
    ready(runtime);
    let (_, token) = prompt(runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");
}

fn summary_of(batch: &AdapterBatch) -> String {
    batch
        .events()
        .iter()
        .find_map(|event| match event {
            SharedEvent::PermissionRequest {
                payload: NormalizedPayload::PermissionRequest { summary, .. },
                ..
            } => Some(summary.as_str().to_owned()),
            _ => None,
        })
        .expect("a permission request is published")
}

#[test]
fn live_reject_only_permission_is_published_without_approval() {
    let mut runtime = ClaudeRuntime::fixture();
    opened(&mut runtime);
    started(&mut runtime, "tool-reject-only");
    let batch = runtime.ingest(&frame(json!({
        "id":"permission-reject-only","method":"session/request_permission","params":{
            "sessionId":"claude-session-1","toolCall":{"toolCallId":"tool-reject-only","title":"bash","kind":"execute","rawInput":{"command":"pwd"}},
            "options":[{"optionId":"reject","kind":"reject_once","name":"Reject"}]
        }
    })), &Mapper).expect("reject-only request is live");
    let offered = batch
        .events()
        .iter()
        .find_map(|event| match event {
            SharedEvent::PermissionRequest {
                payload:
                    NormalizedPayload::PermissionRequest {
                        offered_option_kinds,
                        ..
                    },
                ..
            } => offered_option_kinds.as_ref(),
            _ => None,
        })
        .expect("permission published");
    assert_eq!(
        offered.as_slice(),
        &[sage_acp_protocol::intent::PermissionOptionKind::RejectOnce]
    );
    let request_id = batch
        .events()
        .iter()
        .find_map(|event| match event {
            SharedEvent::PermissionRequest {
                external_request_id,
                ..
            } => Some(external_request_id.as_str()),
            _ => None,
        })
        .unwrap();
    let denial = runtime
        .command(
            crate::acp::bindings::adapter::AdapterCommand::ResolvePermission {
                request_id: sage_acp_protocol::contract::SageServerRequestId::parse(request_id)
                    .unwrap(),
                decision: sage_acp_protocol::permissions::PermissionDecision::Deny,
            },
        )
        .expect("reject-only request can be denied");
    assert!(!denial.frame().payload().is_empty());
}

#[test]
fn advertised_claude_ask_emits_prompt_then_requires_permission() {
    let capabilities =
        crate::acp::bindings::access_policy::access_modes("claude-agent-acp", "0.76.0", true);
    assert_eq!(
        capabilities[0].id,
        sage_acp_protocol::access_mode::SessionAccessMode::Ask
    );
    assert!(capabilities[0].fallback);
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "run a tool").unwrap(),
            client_message_id: RequiredText::new("client", "admitted-ask").unwrap(),
            access_mode: capabilities[0].id,
        })
        .unwrap();
    let wire: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(wire["method"], "session/prompt");
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    started(&mut runtime, "admitted-tool");
    let batch = runtime
        .ingest(
            &permission_frame("admitted-tool", "admitted-permission", "pwd"),
            &Mapper,
        )
        .unwrap();
    assert!(batch.egress().is_empty());
    assert!(
        batch
            .events()
            .iter()
            .any(|event| matches!(event, SharedEvent::PermissionRequest { .. }))
    );
}

#[test]
fn the_permission_relabel_reaches_the_durable_tool_card() {
    let mut runtime = ClaudeRuntime::fixture();
    opened(&mut runtime);
    started(&mut runtime, "tool-input");
    let batch = runtime
        .ingest(
            &permission_frame("tool-input", "permission-input", "cargo test --lib"),
            &Mapper,
        )
        .expect("permission");
    let events = batch.events();
    let kinds = events
        .iter()
        .find_map(|event| match event {
            SharedEvent::PermissionRequest {
                payload:
                    NormalizedPayload::PermissionRequest {
                        offered_option_kinds,
                        ..
                    },
                ..
            } => offered_option_kinds.as_ref(),
            _ => None,
        })
        .expect("Claude offered kinds are persisted");
    assert_eq!(
        kinds.as_slice(),
        &[
            sage_acp_protocol::intent::PermissionOptionKind::AllowOnce,
            sage_acp_protocol::intent::PermissionOptionKind::RejectOnce
        ]
    );
    let card = events
        .iter()
        .position(|event| {
            matches!(
                event,
                SharedEvent::Item {
                    payload: NormalizedPayload::ToolCall {
                        input_summary: Some(_),
                        ..
                    },
                    ..
                }
            )
        })
        .expect("the tool card input summary is persisted");
    let request = events
        .iter()
        .position(|event| matches!(event, SharedEvent::PermissionRequest { .. }))
        .expect("the permission request is published");
    assert!(
        card < request,
        "the tool card is ordered before the permission"
    );
    let SharedEvent::Item {
        payload: NormalizedPayload::ToolCall { input_summary, .. },
        ..
    } = &events[card]
    else {
        panic!("tool call card")
    };
    assert_eq!(
        input_summary.as_ref().expect("input summary").as_str(),
        "cargo test --lib"
    );
}

#[test]
fn a_partially_redacted_command_is_replaced_by_an_explicit_placeholder() {
    let mut runtime = ClaudeRuntime::fixture();
    opened(&mut runtime);
    started(&mut runtime, "tool-mask");
    let batch = runtime
        .ingest(
            &permission_frame(
                "tool-mask",
                "permission-mask",
                "Write-Output C:\\secret.txt;Remove-Item all",
            ),
            &Mapper,
        )
        .expect("permission");
    let summary = summary_of(&batch);
    assert_eq!(summary, HIDDEN);
    assert!(!summary.contains("Remove-Item"));
    assert!(!summary.contains("[redacted]"));
}

#[test]
fn an_escape_heavy_command_under_the_source_bound_degrades_instead_of_failing() {
    let mut runtime = ClaudeRuntime::fixture();
    opened(&mut runtime);
    started(&mut runtime, "tool-escape");
    let command = "\"".repeat(65_000);
    let batch = runtime
        .ingest(
            &permission_frame("tool-escape", "permission-escape", &command),
            &Mapper,
        )
        .expect("permission");
    assert_eq!(summary_of(&batch), TOO_LONG);
}

#[test]
fn the_largest_displayable_command_still_serializes_inside_the_permission_payload_cap() {
    let mut runtime = ClaudeRuntime::fixture();
    opened(&mut runtime);
    started(&mut runtime, "tool-limit");
    let command = "\"".repeat(32_512);
    let batch = runtime
        .ingest(
            &permission_frame("tool-limit", "permission-limit", &command),
            &Mapper,
        )
        .expect("permission");
    assert_eq!(summary_of(&batch), command);
    let payload = batch
        .events()
        .iter()
        .find_map(|event| match event {
            SharedEvent::PermissionRequest { payload, .. } => Some(payload),
            _ => None,
        })
        .expect("a permission request is published");
    let serialized = serde_json::to_vec(payload).expect("serialized");
    assert!(
        serialized.len() <= sage_core::limits::LIMITS.wire.permission_payload_bytes as usize,
        "serialized permission payload stays inside the cap"
    );
}
