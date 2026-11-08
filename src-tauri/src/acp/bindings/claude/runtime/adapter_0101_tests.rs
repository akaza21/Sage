//! claude-agent-acp 0.101.0 / ACP SDK 2.4.0 behaviour, replayed from the
//! frames in `testdata/acp/claude/derived-0101.json`. Those frames are derived
//! from the package code, not captured live; see that file's `provenance`.

use super::*;
use crate::acp::{
    goal::NativeRpcReply,
    lifecycle::ServerRequestState,
    runtime::{ActivationIntent, ActivationMode, ActivationPump, ActivationPumpError},
};

#[path = "adapter_0101_tests/cancel.rs"]
mod cancel;
#[path = "adapter_0101_tests/fork.rs"]
mod fork;
#[path = "adapter_0101_tests/mode.rs"]
mod mode;

const FORK: &str = "5e0c7a91-3b6d-4f28-a1c4-8d2f9e6b0a37";

fn derived(name: &str) -> Value {
    let document: Value = serde_json::from_slice(include_bytes!(
        "../../../../../testdata/acp/claude/derived-0101.json"
    ))
    .unwrap();
    assert_eq!(document["provenance"], "derived");
    document["frames"][name]["frame"].clone()
}

fn answer(name: &str, id: u64) -> Frame {
    let mut value = derived(name);
    value["id"] = json!(id);
    frame(value)
}

fn payload(egress: &AdapterEgress) -> Value {
    serde_json::from_slice(egress.frame().payload()).unwrap()
}

fn initialize_0101(runtime: &mut ClaudeRuntime) {
    let egress = runtime.command(AdapterCommand::Initialize).unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    runtime.ingest(&answer("initialize", id), &Mapper).unwrap();
}

/// Sends `session/new` and returns the batch its answer produces.
fn new_session(runtime: &mut ClaudeRuntime, result: Frame) -> AdapterBatch {
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    let mut value: Value = serde_json::from_slice(result.as_bytes()).unwrap();
    value["id"] = json!(id);
    runtime.ingest(&frame(value), &Mapper).unwrap()
}

fn current_mode(runtime: &ClaudeRuntime) -> String {
    runtime
        .session_config()
        .unwrap()
        .modes
        .unwrap()
        .current_mode_id
        .as_str()
        .to_owned()
}

/// A 0.101.0 session in Sage's mode, with no mode catalog to correct.
fn ready_0101(runtime: &mut ClaudeRuntime) {
    initialize_0101(runtime);
    let batch = new_session(
        runtime,
        frame(
            json!({"jsonrpc":"2.0","id":0,"result":{"sessionId":"claude-session-1","modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"}]}}}),
        ),
    );
    assert!(matches!(batch.events(), [SharedEvent::SessionReady { .. }]));
}

#[test]
fn the_derived_0101_advertisement_negotiates_the_0760_capability_snapshot() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let capabilities = runtime.capabilities().unwrap();
    assert!(capabilities.resume_session);
    assert!(capabilities.close_session);
    assert!(capabilities.native_session_fork);
    assert!(capabilities.prompt_resources);
    assert!(!capabilities.load_session);
}

fn activation_intent(
    mode: ActivationMode,
    ceiling: crate::acp::intent::CapabilitySnapshotV1,
) -> ActivationIntent {
    ActivationIntent {
        binding_change: None,
        binding_digest: [7; 32],
        artifact_digest: "d".repeat(64),
        project_id: crate::acp::contract::SageProjectId::parse("project-one").unwrap(),
        chat_id: crate::acp::contract::SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-claude-runtime").unwrap(),
        runtime_epoch: RuntimeEpoch::new(3).unwrap(),
        mode,
        curated_ceiling: ceiling.clone(),
        sage_policy: ceiling,
    }
}

fn line(value: Value) -> Vec<u8> {
    format!("{value}\n").into_bytes()
}

#[test]
fn a_resume_of_a_missing_native_session_is_a_typed_activation_error() {
    use super::resume_regression_tests::{Journal, Sink};
    let mut probe = ClaudeRuntime::fixture();
    initialize_0101(&mut probe);
    let ceiling = probe.capabilities().unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let intent = activation_intent(
            ActivationMode::Resume {
                external_session_id: ExternalId::new(
                    "external",
                    "06511af0-f6d1-4ac4-9b4a-81ddbcb007e2",
                )
                .unwrap(),
                native_root: r"C:\fixture\root".into(),
                model: None,
            },
            ceiling,
        );
        let mut pump =
            ActivationPump::new(ClaudeRuntime::fixture(), Journal, Mapper, intent).unwrap();
        pump.begin().unwrap();
        pump.flush_one(&mut Sink).unwrap();
        pump.push_stdout_chunk(&line(derived("initialize")), 1)
            .await
            .unwrap();
        pump.flush_one(&mut Sink).unwrap();
        let missing = pump
            .push_stdout_chunk(&line(derived("resumeMissing")), 2)
            .await;
        assert!(matches!(
            missing,
            Err(ActivationPumpError::Adapter(
                AdapterRuntimeError::NativeSessionMissing
            ))
        ));
    });
}

#[test]
fn only_a_resource_not_found_for_the_resumed_session_means_it_is_missing() {
    let resume = |answer: Value| {
        let mut runtime = ClaudeRuntime::fixture();
        initialize_0101(&mut runtime);
        let egress = runtime
            .command(AdapterCommand::ResumeSession {
                external_session_id: ExternalId::new("external", "claude-session-1").unwrap(),
                native_root: r"C:\fixture\root".into(),
                model: None,
            })
            .unwrap();
        let mut answer = answer;
        answer["jsonrpc"] = json!("2.0");
        answer["id"] = json!(request_id(&egress));
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .unwrap();
        runtime.ingest(&frame(answer), &Mapper).err()
    };
    assert_eq!(
        resume(json!({"error":{"code":-32002,"message":"Resource not found"}})),
        Some(AdapterRuntimeError::NativeSessionMissing)
    );
    assert_eq!(
        resume(
            json!({"error":{"code":-32002,"message":"Resource not found",
            "data":{"sessionId":"claude-session-1"}}})
        ),
        Some(AdapterRuntimeError::NativeSessionMissing)
    );
    // 0.76.0 reports a missing session as a generic internal error.
    assert_eq!(
        resume(json!({"error":{"code":-32603,"message":"Internal error",
            "data":{"details":"Session not found"}}})),
        Some(AdapterRuntimeError::Frame)
    );
    assert_eq!(
        resume(
            json!({"error":{"code":-32002,"message":"Resource not found",
            "data":{"sessionId":"claude-session-9"}}})
        ),
        Some(AdapterRuntimeError::Frame)
    );
}
