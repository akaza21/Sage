use serde_json::{Value, json};

use super::*;
use crate::acp::{
    bindings::adapter::{AdapterCommand, AdapterRuntimeError, EgressResolution, HarnessAdapter},
    contract::{RequiredText, SageRootId, SageServerRequestId},
    intent::{ExternalId, Prompt, SafeResourceRef},
    lifecycle::{ToolState, TurnState},
    transport::FrameReader,
};

struct Mapper;

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

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).expect("json");
    bytes.push(b'\n');
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(FrameReader::new(bytes.as_slice()).read_frame())
        .expect("valid frame")
        .expect("complete frame")
}

fn request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<Value>(egress.frame().payload()).expect("json")["id"]
        .as_u64()
        .expect("numeric id")
}

fn initialize(runtime: &mut ClaudeRuntime) {
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
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"claude-session-1"}})),
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
        })
        .expect("prompt command");
    let id = request_id(&egress);
    (id, egress.into_token())
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
    let SharedEvent::Item { id: identity, .. } = &update.events()[0] else {
        panic!("item")
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
        [SharedEvent::SessionReady { loaded: true, .. }]
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
        [SharedEvent::Turn {
            state: TurnState::Completed,
            ..
        }]
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
fn permission_denial_requires_commit_then_queue_then_write_and_replays_boundedly() {
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
    assert_eq!(batch.egress().len(), 1);
    assert!(batch.actual_bytes() <= LIMITS.queues.journal_bytes_per_transaction as usize);
    let token = batch.egress()[0].token().clone();
    assert!(token.is_mandatory());
    let SharedEvent::PermissionRequest { item_id, .. } = &batch.events()[0] else {
        panic!("permission request")
    };
    assert_eq!(
        item_id.external_id.as_str(),
        token.deny_request().expect("deny identity").0
    );
    assert_eq!(
        runtime.resolve_egress(token.clone(), EgressResolution::Queued),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    );
    assert_eq!(
        runtime.resolve_egress(token.clone(), EgressResolution::QueueRejected),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    );
    assert_eq!(
        runtime.resolve_egress(token.clone(), EgressResolution::DeliveryUnknown),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    );
    runtime
        .commit_mandatory_egress(&token)
        .expect("durable commit");
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued after commit");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written after queue");

    let duplicate = runtime
        .ingest(&frame(permission), &Mapper)
        .expect("duplicate permission");
    assert!(matches!(
        duplicate.events(),
        [SharedEvent::IgnoredDuplicate]
    ));
    assert!(duplicate.egress()[0].token().is_mandatory());
}

#[test]
fn unresolved_permission_decision_reports_missing_intent() {
    let mut runtime = ClaudeRuntime::fixture();
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("request-claude-missing").expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::Deny,
        }),
        Err(AdapterRuntimeError::PermissionIntent)
    ));
}

#[test]
fn numeric_and_string_permission_ids_cannot_alias_and_preserve_wire_type() {
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
            .reserve_denial(&id, id.clone(), Value::String(id.clone()))
            .expect("within bound");
    }
    assert_eq!(blocked.denials.len(), limit);
    assert_eq!(
        blocked.reserve_denial(
            "limit-plus-one",
            "limit-plus-one".into(),
            Value::String("limit-plus-one".into())
        ),
        Err(AdapterRuntimeError::MandatoryDenyTeardown)
    );

    let mut pruned = ClaudeRuntime::fixture();
    for index in 0..limit {
        let id = format!("written-{index}");
        pruned
            .reserve_denial(&id, id.clone(), Value::String(id.clone()))
            .expect("within bound");
        pruned.denials.get_mut(&id).expect("record").state = DenialState::Written;
    }
    pruned
        .reserve_denial(
            "replacement",
            "replacement".into(),
            Value::String("replacement".into()),
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
