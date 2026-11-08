use serde_json::{Value, json};

use super::*;
mod slash_commands;
use crate::acp::{
    bindings::adapter::{AdapterCommand, AdapterRuntimeError, EgressResolution, HarnessAdapter},
    contract::{RequiredText, SageRootId, SageServerRequestId},
    intent::{ExternalId, NormalizedPayload, Prompt, SafeResourceRef, ToolCategory},
    lifecycle::{ToolState, TurnState},
    transport::FrameReader,
};

pub(super) struct Mapper;

impl SharedResourceMapper for Mapper {
    fn map_existing(
        &self,
        absolute_path: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        (absolute_path == r"C:\fixture\root\src\lib.rs")
            .then(|| {
                SafeResourceRef::new(
                    SageRootId::parse("root-claude-runtime").expect("root"),
                    "src/lib.rs",
                )
                .expect("resource")
            })
            .ok_or(crate::acp::intent::UnavailableResourceReason::Foreign)
    }
}

pub(super) fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).expect("json");
    bytes.push(b'\n');
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(FrameReader::new(bytes.as_slice()).read_frame())
        .expect("valid frame")
        .expect("complete frame")
}

pub(super) fn request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<Value>(egress.frame().payload()).expect("json")["id"]
        .as_u64()
        .expect("numeric id")
}

pub(super) fn initialize(runtime: &mut ClaudeRuntime) {
    let egress = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize command");
    assert!(!egress.frame().payload().contains(&b'\n'));
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .expect("queued");
    let batch = runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":id,"result":{
                    "protocolVersion":1,
                    "agentCapabilities":{
                        "loadSession":true,
                        "sessionCapabilities":{"resume":{},"close":{}}
                    },
                    "models":[{"id":"claude-reviewed","name":"Claude reviewed"}]
                }
            })),
            &Mapper,
        )
        .expect("initialize response");
    assert!(matches!(batch.events()[0], SharedEvent::Initialized { .. }));
    assert!(matches!(batch.events()[1], SharedEvent::Models { .. }));
    let capabilities = runtime.capabilities().expect("capabilities");
    assert!(capabilities.resume_session);
    assert!(!capabilities.load_session);
    assert!(capabilities.close_session);
}

fn ready(runtime: &mut ClaudeRuntime) {
    initialize(runtime);
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .expect("new session");
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .expect("written");
    let batch = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"claude-session-1","modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"}]}}})),
            &Mapper,
        )
        .expect("session response");
    assert!(matches!(
        batch.events(),
        [SharedEvent::SessionReady { loaded: false, .. }]
    ));
}

fn prompt(runtime: &mut ClaudeRuntime) -> (u64, EgressToken) {
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-message-1").expect("client"),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .expect("prompt command");
    let id = request_id(&egress);
    (id, egress.into_token())
}

#[test]
fn mcp_session_wire_preserves_empty_array_and_injects_only_admitted_servers() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize(&mut runtime);
    let empty = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .unwrap();
    let text = std::str::from_utf8(empty.frame().payload()).unwrap();
    assert!(text.contains("\"mcpServers\":[]"), "{text}");

    let mut runtime = ClaudeRuntime::fixture();
    runtime.mcp = crate::acp::mcp::McpAdmission::for_test(vec![
        crate::acp::mcp::McpServerDescriptor::stdio_for_test(
            "files",
            r"C:\tools\mcp.exe",
            &[],
            vec![],
        ),
        crate::acp::mcp::McpServerDescriptor::http_for_test("remote", "https://example.com/mcp"),
    ])
    .unwrap();
    initialize(&mut runtime);
    let injected = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .unwrap();
    let value: Value = serde_json::from_slice(injected.frame().payload()).unwrap();
    assert_eq!(
        value["params"]["mcpServers"],
        json!([{
            "name":"files","command":r"C:\tools\mcp.exe","args":[],"env":[]
        }])
    );
}

#[test]
fn initialize_new_prompt_update_terminal_cancel_and_close_preserve_intent() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");

    let update = runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"agent_message_chunk","itemId":"message-1",
                    "content":{"type":"text","text":"safe result"}
                }
            }})),
            &Mapper,
        )
        .expect("update");
    let [
        SharedEvent::Turn {
            state: TurnState::Running,
            ..
        },
        SharedEvent::Item { id: identity, .. },
    ] = update.events()
    else {
        panic!("expected the turn to start before its first item")
    };
    assert_eq!(identity.binding_digest, [7; 32]);
    assert_eq!(identity.runtime_epoch.get(), 3);
    assert_eq!(identity.external_id.as_str(), "message-1");
    assert_eq!(
        identity.turn_id.as_ref().expect("turn").as_str(),
        format!("prompt-{id}")
    );
    assert_eq!(
        identity
            .turn_client_message_id
            .as_ref()
            .expect("client message")
            .as_str(),
        "client-message-1"
    );

    let cancel = runtime.command(AdapterCommand::Cancel).expect("cancel");
    assert!(cancel.token().is_control() && cancel.token().is_cancel());
    assert_eq!(
        serde_json::from_slice::<Value>(cancel.frame().payload()).expect("cancel json")["method"],
        "session/cancel"
    );
    runtime
        .resolve_egress(cancel.into_token(), EgressResolution::Written)
        .expect("cancel written");

    let terminal = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"cancelled"}})),
            &Mapper,
        )
        .expect("terminal");
    assert!(matches!(
        terminal.events(),
        [SharedEvent::Turn {
            state: TurnState::Cancelled,
            ..
        }]
    ));

    let close = runtime
        .command(AdapterCommand::CloseSession)
        .expect("close");
    let close_id = request_id(&close);
    let closed = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":close_id,"result":{}})),
            &Mapper,
        )
        .expect("close response");
    assert!(matches!(closed.events(), [SharedEvent::Acknowledged]));
}

#[test]
fn a_real_claude_turn_correlates_before_its_items_and_snapshots_usage_off_the_item_stream() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("prompt written");

    let usage = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"claude-session-1",
                "update":{"sessionUpdate":"usage_update","used":23753,"size":1000000}
            }})),
            &Mapper,
        )
        .expect("usage update");
    assert!(
        matches!(
            usage.events(),
            [
                SharedEvent::Turn {
                    state: TurnState::Running,
                    ..
                },
                SharedEvent::Absorbed
            ]
        ),
        "{:?}",
        usage.events()
    );

    let message = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"claude-session-1",
                "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}
            }})),
            &Mapper,
        )
        .expect("agent message");
    assert!(
        matches!(message.events(), [SharedEvent::Item { .. }]),
        "{:?}",
        message.events()
    );

    let terminal = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{
                "stopReason":"end_turn",
                "usage":{"inputTokens":2,"outputTokens":4,"cachedReadTokens":23747,
                         "cachedWriteTokens":0,"totalTokens":23753},
                "_meta":{"quota":{"token_count":{"totalTokens":23753,"inputTokens":2}}}
            }})),
            &Mapper,
        )
        .expect("prompt result carrying vendor _meta and usage");
    let [
        SharedEvent::Usage { payload, .. },
        SharedEvent::Turn {
            state: TurnState::Completed,
            ..
        },
    ] = terminal.events()
    else {
        panic!("{:?}", terminal.events())
    };
    assert_eq!(
        *payload,
        NormalizedPayload::Usage {
            input_tokens: 2,
            output_tokens: 4,
            context_window: Some(1_000_000),
        }
    );
}

#[test]
fn resume_is_exact_while_load_model_and_model_override_fail_closed() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize(&mut runtime);
    assert!(matches!(
        runtime.command(AdapterCommand::LoadSession {
            external_session_id: ExternalId::new("external", "claude-session-1").expect("id"),
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    assert!(matches!(
        runtime.command(AdapterCommand::ModelPage { cursor: None }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    assert!(runtime.accepts_default_model(Some(
        &RequiredText::new("model", "claude-reviewed").expect("default model")
    )));
    assert!(matches!(
        runtime.command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: Some(RequiredText::new("model", "claude-other").expect("model")),
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));

    let external = ExternalId::new("external", "claude-session-1").expect("id");
    let resume = runtime
        .command(AdapterCommand::ResumeSession {
            external_session_id: external,
            native_root: r"C:\fixture\root".into(),
            model: Some(RequiredText::new("model", "claude-reviewed").expect("model")),
        })
        .expect("resume");
    let id = request_id(&resume);
    let batch = runtime
        .ingest(
            &frame(json!({"id":id,"result":{"sessionId":"claude-session-1"}})),
            &Mapper,
        )
        .expect("resume response");
    assert!(matches!(
        batch.events(),
        [SharedEvent::SessionReady { loaded: false, .. }]
    ));
}

#[test]
fn prompt_zero_byte_rejection_rolls_back_and_delivery_unknown_is_not_replayed() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (first_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::WriteZeroBytes)
        .expect("zero bytes rollback");
    let (second_id, token) = prompt(&mut runtime);
    assert!(second_id > first_id);
    runtime
        .resolve_egress(token, EgressResolution::DeliveryUnknown)
        .expect("delivery unknown retained without replay");
    assert!(
        runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "must not replay").expect("prompt"),
                client_message_id: RequiredText::new("client", "client-message-2").expect("client"),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .is_err()
    );
    let terminal = runtime
        .ingest(
            &frame(json!({"id":second_id,"result":{"stopReason":"end_turn"}})),
            &Mapper,
        )
        .expect("correlated response still wins");
    assert!(matches!(
        terminal.events(),
        [
            SharedEvent::Turn {
                state: TurnState::Running,
                ..
            },
            SharedEvent::Turn {
                state: TurnState::Completed,
                ..
            }
        ]
    ));
}

#[test]
fn initialize_advertises_curated_host_tools_including_browse() {
    let mut runtime = ClaudeRuntime::fixture();
    let egress = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    let value: Value = serde_json::from_slice(egress.frame().payload()).expect("json");
    assert_eq!(value["method"], "initialize");
    assert_eq!(
        value["params"]["clientCapabilities"],
        json!({
            "fs":{"readTextFile":true,"writeTextFile":false},
            "terminal":false,
            "session":{"configOptions":{"boolean":{}}},
            "_meta":{"sage":{"tools":["sage.apply_file", "_sage/browse"]}}
        })
    );
    assert!(
        value["params"]["clientCapabilities"]
            .get("availableTools")
            .is_none()
    );
}

#[test]
fn unknown_host_tool_request_still_fails_closed() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("queued");
    assert!(matches!(
        runtime.ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":"write-1","method":"sage.write_text",
                "params":{"path":"src/lib.rs"}
            })),
            &Mapper,
        ),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    ));
}

#[test]
fn initialize_delivery_unknown_blocks_reissue_until_exact_response_correlates() {
    let mut runtime = ClaudeRuntime::fixture();
    let initialize = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    let id = request_id(&initialize);
    runtime
        .resolve_egress(initialize.into_token(), EgressResolution::DeliveryUnknown)
        .expect("unknown delivery latched");
    assert!(matches!(
        runtime.command(AdapterCommand::Initialize),
        Err(AdapterRuntimeError::Operation)
    ));
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{
                "protocolVersion":1,"agentCapabilities":{}
            }})),
            &Mapper,
        )
        .expect("exact response correlates delivery");
    assert!(matches!(
        runtime.command(AdapterCommand::Initialize),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn allow_once_on_claude_does_not_emit_denial_and_records_resolved_allowed() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("queued");
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":"tool-1",
                    "kind":"execute","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .expect("tool start");
    let permission = json!({
        "id":"permission-1","method":"session/request_permission","params":{
            "sessionId":"claude-session-1","toolCall":{"toolCallId":"tool-1"},
            "options":[{"optionId":"allow","name":"Allow"}]
        }
    });
    let batch = runtime
        .ingest(&frame(permission.clone()), &Mapper)
        .expect("permission");
    assert!(matches!(
        batch.events(),
        [SharedEvent::PermissionRequest { .. }]
    ));
    assert!(batch.egress().is_empty());
    let SharedEvent::PermissionRequest {
        payload:
            NormalizedPayload::PermissionRequest {
                offered_option_kinds: Some(kinds),
                ..
            },
        ..
    } = &batch.events()[0]
    else {
        panic!("offered options")
    };
    assert!(
        kinds
            .as_slice()
            .contains(&crate::acp::intent::PermissionOptionKind::AllowOnce)
    );
    assert!(batch.actual_bytes() <= LIMITS.queues.journal_bytes_per_transaction as usize);
    let SharedEvent::PermissionRequest {
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission request")
    };
    assert_eq!(external_request_id.as_str(), "string:permission-1");
    let allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(external_request_id.as_str()).expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
        })
        .expect("allow once");
    assert!(!allow.token().is_mandatory());
    assert_eq!(
        allow.token().allow_request(),
        Some(external_request_id.as_str())
    );
    let response: Value = serde_json::from_slice(allow.frame().payload()).expect("response");
    assert_eq!(response["id"], json!("permission-1"));
    assert_eq!(response["result"]["outcome"]["optionId"], json!("allow"));
    runtime
        .resolve_egress(allow.token().clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(allow.into_token(), EgressResolution::Written)
        .expect("written");

    let duplicate = runtime
        .ingest(&frame(permission), &Mapper)
        .expect("duplicate permission");
    assert!(matches!(
        duplicate.events(),
        [SharedEvent::IgnoredDuplicate]
    ));
    assert!(duplicate.egress().is_empty());
}

#[test]
fn claude_always_allow_selects_the_native_option_id() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":"tool-1",
                    "kind":"execute","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .unwrap();
    let permission = json!({
        "id":"permission-always","method":"session/request_permission","params":{
            "sessionId":"claude-session-1","toolCall":{"toolCallId":"tool-1"},
            "options":[{"optionId":"native-always","name":"Allow always"}]
        }
    });
    let batch = runtime.ingest(&frame(permission), &Mapper).unwrap();
    let SharedEvent::PermissionRequest {
        payload:
            NormalizedPayload::PermissionRequest {
                offered_option_kinds: Some(kinds),
                ..
            },
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission options")
    };
    assert_eq!(
        kinds.as_slice(),
        &[crate::acp::intent::PermissionOptionKind::AllowAlways]
    );
    let reply = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(external_request_id.as_str()).unwrap(),
            decision: crate::acp::runtime::permissions::PermissionDecision::AllowAlways,
        })
        .unwrap();
    let response: Value = serde_json::from_slice(reply.frame().payload()).unwrap();
    assert_eq!(response["result"]["outcome"]["optionId"], "native-always");
}

#[test]
fn later_decision_uses_the_shared_helper_not_permission_intent_reject() {
    let mut runtime = ClaudeRuntime::fixture();
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("request-claude-missing").expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::Deny,
        }),
        Err(AdapterRuntimeError::LaterPermissionMiss)
    ));
}

#[test]
fn the_captured_claude_write_approval_survives_runtime_ingest_end_to_end() {
    let tree = std::env::temp_dir().join(format!("sage-hr6-runtime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tree).expect("tree");
    let tree = tree.canonicalize().expect("canonical");
    let live = crate::acp::resource_mapper::NativeResourceMapper::new([
        crate::acp::resource_mapper::ConfiguredResourceRoot {
            root_id: SageRootId::parse("root-claude-runtime").expect("root"),
            canonical_native_root: tree.clone(),
        },
    ])
    .expect("mapper");
    let target = tree.join("hr6-permission.txt");
    let path = target.to_str().expect("path").to_owned();
    let tool = "toolu_01VxPvQHiywMnCJRonXWy9oU";
    let body = "hr6 permission granted\n";

    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("queued");

    let mut seen: Vec<SharedEvent> = Vec::new();
    let replay = |runtime: &mut ClaudeRuntime, update: Value, seen: &mut Vec<SharedEvent>| {
        let batch = runtime
            .ingest(
                &frame(json!({"method":"session/update","params":{
                    "sessionId":"claude-session-1","update":update
                }})),
                &live,
            )
            .expect("session update");
        seen.extend(batch.events().iter().cloned());
    };

    replay(
        &mut runtime,
        json!({
            "_meta": {"claudeCode": {"toolName": "Write"}},
            "toolCallId": tool,
            "sessionUpdate": "tool_call",
            "rawInput": {},
            "status": "pending",
            "title": "Preparing file",
            "kind": "edit",
            "content": [],
            "locations": [],
        }),
        &mut seen,
    );
    replay(
        &mut runtime,
        json!({
            "_meta": {"claudeCode": {"toolName": "Write"}},
            "toolCallId": tool,
            "sessionUpdate": "tool_call_update",
            "rawInput": {"file_path": path, "content": body},
            "title": "Write hr6-permission.txt",
            "kind": "edit",
            "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
            "locations": [{"path": path}],
        }),
        &mut seen,
    );

    let permission = json!({
        "id": "permission-hr6", "method": "session/request_permission", "params": {
            "sessionId": "claude-session-1",
            "toolCall": {
                "toolCallId": tool,
                "status": "pending",
                "title": "Write hr6-permission.txt",
                "kind": "edit",
                "rawInput": {"file_path": path, "content": body},
                "content": [{"type": "diff", "path": path, "oldText": Value::Null, "newText": body}],
                "locations": [{"path": path}]
            },
            "_meta": {"permission": {"version": 1, "title": "Write hr6-permission.txt"}},
            "options": [
                {"optionId": "allow-once", "name": "Yes", "kind": "allow_once"},
                {"optionId": "reject", "name": "No", "kind": "reject_once"}
            ]
        }
    });
    let batch = runtime
        .ingest(&frame(permission), &live)
        .expect("permission request");
    let SharedEvent::PermissionRequest {
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission request")
    };
    let allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(external_request_id.as_str()).expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
        })
        .expect("allow once");
    runtime
        .resolve_egress(allow.token().clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(allow.into_token(), EgressResolution::Written)
        .expect("written");

    std::fs::write(&target, body).expect("write");
    replay(
        &mut runtime,
        json!({
            "_meta": {"claudeCode": {"toolResponse": {"type": "create", "filePath": path}, "toolName": "Write"}},
            "toolCallId": tool,
            "sessionUpdate": "tool_call_update",
        }),
        &mut seen,
    );
    replay(
        &mut runtime,
        json!({
            "_meta": {"claudeCode": {"toolName": "Write"}},
            "toolCallId": tool,
            "sessionUpdate": "tool_call_update",
            "status": "completed",
            "rawOutput": "File created successfully",
        }),
        &mut seen,
    );

    let _ = std::fs::remove_dir_all(&tree);
    let pairs = seen
        .iter()
        .enumerate()
        .filter_map(|(index, event)| match event {
            SharedEvent::FilePair { id, .. } => Some((index, id.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(pairs.len(), 1);
    let (index, key) = &pairs[0];
    let SharedEvent::Item { id, .. } = &seen[index - 1] else {
        panic!("a file pair must immediately follow its item")
    };
    assert_eq!(id, key);
}

#[test]
fn durable_permission_id_correlates_to_claude_jsonrpc_request_id() {
    let (string_token, string_translator, string_wire) =
        permission_identity(Some(&Value::String("1".into()))).expect("string id");
    let (number_token, number_translator, number_wire) =
        permission_identity(Some(&json!(1))).expect("number id");
    assert_ne!(string_token, number_token);
    assert_eq!(string_translator, number_translator);
    assert!(string_wire.is_string());
    assert!(number_wire.is_number());
}

#[test]
fn notification_delivery_unknown_poison_prevents_automatic_reissue() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("prompt queued");
    let cancel = runtime.command(AdapterCommand::Cancel).expect("cancel");
    runtime
        .resolve_egress(cancel.into_token(), EgressResolution::DeliveryUnknown)
        .expect("delivery unknown latched");
    assert!(matches!(
        runtime.command(AdapterCommand::Cancel),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn denial_ledger_is_exactly_bounded_and_prunes_only_oldest_written_tombstone() {
    let limit = LIMITS.queues.server_requests_per_session as usize;
    let mut blocked = ClaudeRuntime::fixture();
    for index in 0..limit {
        let id = format!("pending-{index}");
        blocked
            .reserve_denial(
                &id,
                id.clone(),
                Value::String(id.clone()),
                Some("allow".into()),
                None,
            )
            .expect("within bound");
    }
    assert_eq!(blocked.denials.len(), limit);
    assert_eq!(
        blocked.reserve_denial(
            "limit-plus-one",
            "limit-plus-one".into(),
            Value::String("limit-plus-one".into()),
            Some("allow".into()),
            None,
        ),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    );

    let mut pruned = ClaudeRuntime::fixture();
    for index in 0..limit {
        let id = format!("written-{index}");
        pruned
            .reserve_denial(
                &id,
                id.clone(),
                Value::String(id.clone()),
                Some("allow".into()),
                None,
            )
            .expect("within bound");
        pruned.denials.get_mut(&id).expect("record").state = DenialState::Written;
    }
    pruned
        .reserve_denial(
            "replacement",
            "replacement".into(),
            Value::String("replacement".into()),
            Some("allow".into()),
            None,
        )
        .expect("written tombstone pruned");
    assert_eq!(pruned.denials.len(), limit);
    assert!(!pruned.denials.contains_key("written-0"));
    assert!(pruned.denials.contains_key("replacement"));
}

#[test]
fn foreign_session_malformed_and_unknown_messages_are_deterministic() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("queued");
    assert!(matches!(
        runtime.ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"foreign","update":{
                    "sessionUpdate":"agent_message_chunk",
                    "content":{"type":"text","text":"foreign"}
                }
            }})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::Frame)
    ));
    let unknown = runtime
        .ingest(
            &frame(json!({"method":"session/unknown","params":{}})),
            &Mapper,
        )
        .expect("unknown notification fallback");
    assert!(matches!(unknown.events(), [SharedEvent::Unsupported(_)]));
    assert!(matches!(
        runtime.ingest(
            &frame(json!({"id":"unknown-request","method":"session/unknown","params":{}})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    ));
    assert!(matches!(
        runtime.ingest(&frame(json!({"id":999,"result":{}})), &Mapper),
        Err(AdapterRuntimeError::Frame)
    ));
}

#[test]
fn admitted_session_can_request_namespaced_sage_browse_during_a_turn() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Queued)
        .expect("queued");
    let opened = runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":"browse-1","method":"_sage/browse",
                "params":{"sessionId":"claude-session-1","url":"http://127.0.0.1:5173/app"}
            })),
            &Mapper,
        )
        .expect("browse");
    assert!(matches!(
        &opened.events()[0],
        SharedEvent::Item {
            payload: NormalizedPayload::ToolCall {
                category: ToolCategory::Fetch,
                input_summary: Some(summary),
                ..
            },
            ..
        } if summary.as_str() == "http://127.0.0.1:5173/app"
    ));
    let reply: Value = serde_json::from_slice(opened.egress()[0].frame().payload()).expect("reply");
    assert_eq!(reply["result"]["opened"], true);
    assert_eq!(reply["result"]["href"], "http://127.0.0.1:5173/app");

    let rejected = runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":"browse-2","method":"_sage/browse",
                "params":{"sessionId":"claude-session-1","url":"https://example.com/"}
            })),
            &Mapper,
        )
        .expect("rejected browse");
    assert!(rejected.events().is_empty());
    let closed: Value =
        serde_json::from_slice(rejected.egress()[0].frame().payload()).expect("closed");
    assert_eq!(closed["result"]["opened"], false);

    let foreign = runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":"browse-3","method":"_sage/browse",
                "params":{"sessionId":"other-session","url":"http://127.0.0.1:4173/"}
            })),
            &Mapper,
        )
        .expect("foreign session");
    assert!(foreign.events().is_empty());
}

#[path = "cancel_contract_tests.rs"]
mod cancel_contract_tests;

#[path = "decline_publication_tests.rs"]
mod decline_publication_tests;

#[path = "permission_surface_tests.rs"]
mod permission_surface_tests;

#[path = "permission_terminal_identity_tests.rs"]
mod permission_terminal_identity_tests;

#[test]
fn an_auth_status_update_is_understood_rather_than_reported_unsupported() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(
                json!({"method":"_auth/status_update","params":{"authStatus":{"kind":"none","label":"Not logged in"}}}),
            ),
            &Mapper,
        )
        .expect("auth status update");
    assert!(matches!(batch.events(), [SharedEvent::Absorbed]));
}

#[test]
fn an_auth_status_request_still_fails_closed_like_any_unknown_request() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    assert!(matches!(
        runtime.ingest(
            &frame(json!({
                "id": "auth-1",
                "method": "_auth/status_update",
                "params": {"authStatus": {"kind": "none"}}
            })),
            &Mapper,
        ),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    ));
}

#[test]
fn a_signed_out_harness_refuses_the_prompt_instead_of_sending_it() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"method":"_auth/status_update","params":{"authStatus":{"kind":"none"}}})),
            &Mapper,
        )
        .expect("auth status update");
    assert!(matches!(
        runtime.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-message-1").expect("client"),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        }),
        Err(AdapterRuntimeError::SignInRequired)
    ));
}

#[test]
fn a_later_signed_in_status_restores_the_prompt_path() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"method":"_auth/status_update","params":{"authStatus":{"kind":"none"}}})),
            &Mapper,
        )
        .expect("signed out");
    runtime
        .ingest(
            &frame(
                json!({"method":"_auth/status_update","params":{"authStatus":{"kind":"subscription","label":"Max"}}}),
            ),
            &Mapper,
        )
        .expect("signed in");
    let (_, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
}

#[test]
fn an_authentication_refusal_costs_one_turn_and_never_a_second() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    assert!(matches!(
        runtime.ingest(
            &frame(
                json!({"id": id, "error": {"code": -32000, "message": "Authentication required"}}),
            ),
            &Mapper,
        ),
        Err(AdapterRuntimeError::SignInRequired)
    ));
    assert!(matches!(
        runtime.command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-message-2").expect("client"),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        }),
        Err(AdapterRuntimeError::SignInRequired)
    ));
    runtime
        .ingest(
            &frame(
                json!({"method":"_auth/status_update","params":{"authStatus":{"kind":"subscription","label":"Max"}}}),
            ),
            &Mapper,
        )
        .expect("signed in");
    let (_, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("prompt after sign-in");
}
#[path = "p01_tests.rs"]
mod p01_tests;

#[path = "browser_permission_regression_tests.rs"]
mod browser_permission_regression_tests;

#[path = "resume_regression_tests.rs"]
mod resume_regression_tests;

#[path = "adapter_0101_tests.rs"]
mod adapter_0101_tests;

#[test]
fn prompt_files_are_refused_without_capability_and_encoded_when_advertised() {
    use crate::acp::attached_context::files::{Resource, Resources};
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let mut prompt = Prompt::new("prompt", "inspect").unwrap();
    prompt.resources = Resources(std::sync::Arc::new(vec![Resource {
        id: "resource-1".into(),
        name: "one.txt".into(),
        bytes: b"snapshot content".to_vec(),
    }]));
    let command = || AdapterCommand::Prompt {
        prompt: prompt.clone(),
        client_message_id: RequiredText::new("client", "file-prompt").unwrap(),
        access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
    };
    assert!(matches!(
        runtime.command(command()),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    assert!(runtime.active.as_ref().unwrap().turn.is_none());
    runtime.capabilities.as_mut().unwrap().prompt_resources = true;
    let egress = runtime.command(command()).unwrap();
    let value: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(value["method"], "session/prompt");
    assert_eq!(value["params"]["prompt"][1]["type"], "resource");
    assert_eq!(
        value["params"]["prompt"][1]["resource"]["text"],
        "snapshot content"
    );
}
