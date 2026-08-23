use serde_json::{Value, json};

use super::*;
use crate::acp::{
    bindings::{
        adapter::{
            AdapterCommand, AdapterEvent, AdapterRuntimeError, EgressResolution, HarnessAdapter,
        },
        pi::{
            ArtifactCandidate, ArtifactIdentity, CuratedPiBinding, PI_LICENSE, PI_NODE_MINIMUM,
            PI_PACKAGE, PI_RPC_ARGS, PI_SOURCE_COMMIT, PI_VERSION, PiPlatform,
        },
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
    intent::{ExternalId, NormalizedPayload, Prompt},
    transport::IncrementalFrameDecoder,
};

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<crate::acp::intent::SafeResourceRef, crate::acp::intent::UnavailableResourceReason>
    {
        Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}

fn fixture_runtime(epoch: u64) -> PiRuntime {
    PiRuntime::fixture(
        RuntimeEpoch::new(epoch).expect("epoch"),
        SageAgentSessionId::parse(format!("session-pi-{epoch}")).expect("session"),
    )
}

fn frame(value: Value) -> Frame {
    let mut bytes = serde_json::to_vec(&value).expect("json");
    bytes.push(b'\n');
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frame = None;
    for chunk in bytes.chunks(8_192) {
        let next = decoder.push_chunk(chunk).expect("valid frame");
        assert!(frame.is_none());
        frame = next.or(frame);
    }
    frame.expect("complete frame")
}

fn request(egress: &AdapterEgress) -> Value {
    serde_json::from_slice(egress.frame().payload()).expect("request json")
}

fn deliver(runtime: &mut PiRuntime, egress: AdapterEgress) -> Value {
    let value = request(&egress);
    let token = egress.into_token();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("written");
    value
}

fn response(request: &Value, data: Value) -> Frame {
    frame(json!({
        "type":"response",
        "id":request["id"],
        "command":request["type"],
        "success":true,
        "data":data
    }))
}

fn initialize(runtime: &mut PiRuntime) {
    let state = runtime
        .command(AdapterCommand::Initialize)
        .expect("initialize");
    assert_eq!(request(&state)["type"], "get_state");
    let state = deliver(runtime, state);
    let batch = runtime
        .ingest(
            &response(
                &state,
                json!({"model":{"provider":"openai","id":"gpt","name":"GPT"}}),
            ),
            &Mapper,
        )
        .expect("state response");
    assert!(batch.events().is_empty());
    assert_eq!(batch.egress().len(), 1);
    let models = request(&batch.egress()[0]);
    let token = batch.egress()[0].token().clone();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("models queued");
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .expect("models written");
    let batch = runtime
        .ingest(
            &response(
                &models,
                json!({"models":[{"provider":"openai","id":"gpt","name":"GPT"}]}),
            ),
            &Mapper,
        )
        .expect("models response");
    assert!(matches!(
        batch.events(),
        [
            AdapterEvent::Models { .. },
            AdapterEvent::Initialized { .. }
        ]
    ));
    let AdapterEvent::Models { models, .. } = &batch.events()[0] else {
        panic!("models")
    };
    assert_eq!(models.iter().filter(|model| model.is_default).count(), 1);
}

fn ready(runtime: &mut PiRuntime) {
    initialize(runtime);
    let new_session = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .expect("new session");
    let value = deliver(runtime, new_session);
    let batch = runtime
        .ingest(&response(&value, Value::Null), &Mapper)
        .expect("session response");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::SessionReady { .. }]
    ));
}

fn running(runtime: &mut PiRuntime) {
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect read-only").expect("prompt"),
            client_message_id: RequiredText::new("client", "client-pi-1").expect("client"),
        })
        .expect("prompt");
    let value = deliver(runtime, prompt);
    runtime
        .ingest(&response(&value, Value::Null), &Mapper)
        .expect("prompt response");
    let batch = runtime
        .ingest(&frame(json!({"type":"agent_start"})), &Mapper)
        .expect("agent start");
    assert!(matches!(batch.events(), [AdapterEvent::Turn { .. }]));
}

fn identity(path: &str, byte: char) -> ArtifactIdentity {
    ArtifactIdentity {
        relative_path: path.into(),
        sha256: byte.to_string().repeat(64),
        bytes: 42,
    }
}

#[test]
fn exact_candidate_remains_launch_unavailable() {
    let entrypoint = identity("pi/dist/cli.js", 'a');
    let node = identity("node/node.exe", 'b');
    let curated = CuratedPiBinding {
        platform: PiPlatform::WindowsX64,
        entrypoint: entrypoint.clone(),
        runtime: node.clone(),
        node_version: PI_NODE_MINIMUM.into(),
    };
    let candidate = ArtifactCandidate {
        platform: PiPlatform::WindowsX64,
        package_name: PI_PACKAGE.into(),
        package_version: PI_VERSION.into(),
        source_commit: PI_SOURCE_COMMIT.into(),
        license: PI_LICENSE.into(),
        node_version: PI_NODE_MINIMUM.into(),
        entrypoint,
        runtime: node,
        fixed_args: PI_RPC_ARGS.map(str::to_owned).to_vec(),
    };
    let binding = curated
        .validate_candidate(&candidate)
        .expect("research binding");
    assert!(matches!(
        PiRuntime::from_binding(
            &binding,
            [1; 32],
            RuntimeEpoch::new(1).expect("epoch"),
            SageAgentSessionId::parse("session-pi-launch").expect("session")
        ),
        Err(AdapterRuntimeError::LaunchUnavailable)
    ));
}

#[test]
fn initialize_is_strictly_staged_and_delivery_outcomes_do_not_replay() {
    let mut ordered = fixture_runtime(1);
    let first = ordered.command(AdapterCommand::Initialize).expect("first");
    let token = first.into_token();
    assert_eq!(
        ordered.resolve_egress(token.clone(), EgressResolution::Written),
        Err(AdapterRuntimeError::EgressIntent)
    );
    ordered
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    ordered
        .resolve_egress(token, EgressResolution::Written)
        .expect("written after queue");

    let mut rejected = fixture_runtime(2);
    let first = rejected.command(AdapterCommand::Initialize).expect("first");
    rejected
        .resolve_egress(first.into_token(), EgressResolution::QueueRejected)
        .expect("queue rollback");
    assert!(rejected.command(AdapterCommand::Initialize).is_ok());

    let mut zero = fixture_runtime(3);
    let first = zero.command(AdapterCommand::Initialize).expect("first");
    let token = first.into_token();
    zero.resolve_egress(token.clone(), EgressResolution::Queued)
        .expect("queued");
    zero.resolve_egress(token, EgressResolution::WriteZeroBytes)
        .expect("zero-byte rollback");
    assert!(zero.command(AdapterCommand::Initialize).is_ok());

    let mut unknown = fixture_runtime(4);
    let first = unknown.command(AdapterCommand::Initialize).expect("first");
    unknown
        .resolve_egress(first.into_token(), EgressResolution::DeliveryUnknown)
        .expect("unknown recorded");
    assert!(matches!(
        unknown.command(AdapterCommand::Initialize),
        Err(AdapterRuntimeError::Operation)
    ));

    let mut malformed = fixture_runtime(11);
    malformed
        .command(AdapterCommand::Initialize)
        .expect("first");
    assert!(matches!(
        malformed.ingest(&frame(json!("invalid")), &Mapper),
        Err(AdapterRuntimeError::Frame)
    ));
    assert!(matches!(
        malformed.command(AdapterCommand::Initialize),
        Err(AdapterRuntimeError::Operation)
    ));

    let mut forged = fixture_runtime(5);
    forged.command(AdapterCommand::Initialize).expect("first");
    assert!(matches!(
        forged.ingest(
            &frame(json!({
                "type":"response","id":"pi-5-2","command":"get_available_models",
                "success":true,"data":{"models":[]}
            })),
            &Mapper
        ),
        Err(AdapterRuntimeError::Frame)
    ));
}

#[test]
fn lifecycle_models_text_tools_usage_cancel_and_unavailable_commands_are_normalized() {
    let mut runtime = fixture_runtime(6);
    initialize(&mut runtime);
    let capabilities = runtime.capabilities().expect("capabilities");
    assert!(capabilities.new_session && capabilities.prompt && capabilities.cancel);
    assert!(
        !capabilities.resume_session && !capabilities.load_session && !capabilities.close_session
    );
    for command in [
        AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("id", "pi-session").expect("id"),
            native_root: r"C:\repo".into(),
            model: None,
        },
        AdapterCommand::LoadSession {
            external_session_id: ExternalId::new("id", "pi-session").expect("id"),
        },
        AdapterCommand::CloseSession,
    ] {
        assert!(matches!(
            runtime.command(command),
            Err(AdapterRuntimeError::UnsupportedCommand)
        ));
    }
    assert!(matches!(
        runtime.command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("model", "other:model").expect("model")),
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    let session = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\private\must-not-enter-wire".into(),
            model: Some(RequiredText::new("model", "openai:gpt").expect("model")),
        })
        .expect("new session");
    assert!(!String::from_utf8_lossy(session.frame().payload()).contains("private"));
    let value = deliver(&mut runtime, session);
    runtime
        .ingest(&response(&value, Value::Null), &Mapper)
        .expect("session");
    running(&mut runtime);

    let start = runtime
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .expect("text start");
    let AdapterEvent::Item { id, .. } = &start.events()[0] else {
        panic!("item")
    };
    assert_eq!(
        id.turn_client_message_id.as_ref().expect("client").as_str(),
        "client-pi-1"
    );
    assert!(matches!(
        runtime
            .ingest(
                &frame(json!({"type":"message_update","delta":"hello"})),
                &Mapper
            )
            .expect("delta")
            .events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Text { .. },
            ..
        }]
    ));
    let second = runtime
        .ingest(
            &frame(json!({"type":"message_update","delta":" world"})),
            &Mapper,
        )
        .expect("second delta");
    assert!(matches!(
        second.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Text { text },
            ..
        }] if text.as_str() == "hello world"
    ));
    let completed = runtime
        .ingest(&frame(json!({"type":"message_end"})), &Mapper)
        .expect("text end");
    assert!(matches!(
        completed.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Text { text },
            ..
        }] if text.as_str() == "hello world"
    ));
    assert!(matches!(
        runtime
            .ingest(
                &frame(json!({
                    "type":"tool_execution_start","toolCallId":"read-1","toolName":"read"
                })),
                &Mapper
            )
            .expect("tool")
            .events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::ToolCall { .. },
            ..
        }]
    ));
    runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_end","toolCallId":"read-1","isError":false
            })),
            &Mapper,
        )
        .expect("tool end");
    let cancel = runtime.command(AdapterCommand::Cancel).expect("cancel");
    assert!(cancel.token().is_control() && cancel.token().is_cancel());
    assert_eq!(request(&cancel)["type"], "abort");
    deliver(&mut runtime, cancel);
    let terminal = runtime
        .ingest(
            &frame(json!({"type":"agent_end","usage":{"inputTokens":2,"outputTokens":1}})),
            &Mapper,
        )
        .expect("terminal");
    assert!(matches!(
        terminal.events(),
        [AdapterEvent::Usage { .. }, AdapterEvent::Turn { .. }]
    ));
}

#[test]
fn allow_once_on_pi_does_not_emit_denial_and_records_resolved_allowed() {
    let mut runtime = fixture_runtime(7);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"tool_execution_start","toolCallId":"write-1","toolName":"search",
                "args":{"path":"C:\\Users\\private"}
            })),
            &Mapper,
        )
        .expect("permission");
    assert!(matches!(
        batch.events(),
        [AdapterEvent::PermissionRequest {
            payload: NormalizedPayload::PermissionRequest {
                category: crate::acp::intent::ToolCategory::Other,
                ..
            },
            ..
        }]
    ));
    assert!(batch.egress().is_empty());
    let AdapterEvent::PermissionRequest {
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission request")
    };
    assert_eq!(external_request_id.as_str(), "write-1");
    let allow = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("write-1").expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
        })
        .expect("allow once");
    assert!(!allow.token().is_mandatory());
    assert_eq!(allow.token().allow_request(), Some("write-1"));
    assert_ne!(request(&allow)["type"], "abort");
    runtime
        .resolve_egress(allow.token().clone(), EgressResolution::Queued)
        .expect("queued");
    runtime
        .resolve_egress(allow.into_token(), EgressResolution::Written)
        .expect("written");
}

#[test]
fn durable_permission_id_correlates_to_pi_request_id() {
    let mut runtime = fixture_runtime(12);
    ready(&mut runtime);
    running(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"permission_request","requestId":"pi-correlation"
            })),
            &Mapper,
        )
        .expect("permission");
    let AdapterEvent::PermissionRequest {
        external_request_id,
        ..
    } = &batch.events()[0]
    else {
        panic!("permission request")
    };
    assert_eq!(external_request_id.as_str(), "pi-correlation");
}

#[test]
fn later_decision_uses_the_shared_helper_not_permission_intent_reject() {
    let mut runtime = fixture_runtime(11);
    assert!(matches!(
        runtime.command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse("request-pi-missing").expect("request"),
            decision: crate::acp::runtime::permissions::PermissionDecision::Deny,
        }),
        Err(AdapterRuntimeError::LaterPermissionMiss)
    ));
}

#[test]
fn malformed_unknown_and_capacity_plus_one_are_bounded() {
    let mut runtime = fixture_runtime(9);
    assert!(matches!(
        runtime.ingest(&frame(json!("invalid")), &Mapper),
        Err(AdapterRuntimeError::Frame)
    ));
    ready(&mut runtime);
    running(&mut runtime);
    assert!(matches!(
        runtime
            .ingest(&frame(json!({"type":"future_event"})), &Mapper)
            .expect("unknown")
            .events(),
        [AdapterEvent::Unsupported(_)]
    ));

    let mut full = fixture_runtime(10);
    initialize(&mut full);
    for _ in 0..16 {
        full.command(AdapterCommand::ModelPage { cursor: None })
            .expect("bounded request");
    }
    assert!(matches!(
        full.command(AdapterCommand::ModelPage { cursor: None }),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn text_snapshot_plus_one_poison_and_prompt_rollback_clear_state() {
    let mut rollback = fixture_runtime(12);
    ready(&mut rollback);
    let prompt = rollback
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "rollback").expect("prompt"),
            client_message_id: RequiredText::new("client", "rollback-client").expect("client"),
        })
        .expect("prompt");
    rollback
        .resolve_egress(prompt.into_token(), EgressResolution::QueueRejected)
        .expect("rollback");
    assert!(rollback.active_turn.is_none());

    let mut bounded = fixture_runtime(13);
    ready(&mut bounded);
    running(&mut bounded);
    bounded
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .expect("start");
    bounded
        .ingest(
            &frame(json!({"type":"message_update","delta":"x".repeat(262_144)})),
            &Mapper,
        )
        .expect("exact bound");
    assert!(matches!(
        bounded.ingest(
            &frame(json!({"type":"message_update","delta":"x"})),
            &Mapper,
        ),
        Err(AdapterRuntimeError::BatchBytes)
    ));
    assert!(
        bounded
            .active_turn
            .as_ref()
            .is_some_and(|turn| turn.text_snapshot.is_none())
    );
    assert!(matches!(
        bounded.command(AdapterCommand::Cancel),
        Err(AdapterRuntimeError::Operation)
    ));
}

#[test]
fn split_chunk_secrets_and_paths_are_redacted_after_snapshot_join() {
    let mut runtime = fixture_runtime(14);
    ready(&mut runtime);
    running(&mut runtime);
    runtime
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .expect("start");
    let mut rendered = String::new();
    for delta in [
        "Bearer ",
        "token-secret ",
        "C:",
        r"\Users\private ",
        "sk-",
        "private",
    ] {
        let batch = runtime
            .ingest(
                &frame(json!({"type":"message_update","delta":delta})),
                &Mapper,
            )
            .expect("chunk");
        rendered = format!("{:?}", batch.events());
    }
    assert!(rendered.contains("Bearer"));
    assert!(!rendered.contains("token-secret"));
    assert!(!rendered.contains("Users"));
    assert!(!rendered.contains("private"));
    assert!(!rendered.contains("sk-"));
}
