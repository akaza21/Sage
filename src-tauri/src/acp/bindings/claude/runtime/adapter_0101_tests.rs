//! claude-agent-acp 0.101.0 / ACP SDK 2.4.0 behaviour, replayed from the
//! frames in `testdata/acp/claude/derived-0101.json`. Those frames are derived
//! from the package code, not captured live; see that file's `provenance`.

use super::*;
use crate::acp::{
    goal::NativeRpcReply,
    lifecycle::ServerRequestState,
    runtime::{ActivationIntent, ActivationMode, ActivationPump, ActivationPumpError},
};

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
        frame(json!({"jsonrpc":"2.0","id":0,"result":{"sessionId":"claude-session-1"}})),
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

#[test]
fn a_new_session_that_starts_in_auto_is_switched_to_sages_mode_before_it_is_ready() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let batch = new_session(&mut runtime, frame(derived("newSessionInAuto")));
    assert!(
        batch.events().is_empty(),
        "the session is not ready while it runs in auto"
    );
    let [set_mode] = batch.egress() else {
        panic!("one starting-mode request")
    };
    let request = payload(set_mode);
    assert_eq!(request["method"], "session/set_mode");
    assert_eq!(
        request["params"],
        json!({"sessionId":"0b5d3c1e-7a42-4f0e-9d61-2c8e4f7a1b93","modeId":"default"})
    );
    let id = request_id(set_mode);
    runtime
        .resolve_egress(set_mode.token().clone(), EgressResolution::Written)
        .unwrap();
    let ready = runtime
        .ingest(&answer("setModeAccepted", id), &Mapper)
        .unwrap();
    assert!(matches!(
        ready.events(),
        [SharedEvent::SessionReady { loaded: false, .. }]
    ));
    assert_eq!(current_mode(&runtime), "default");
    // The new boolean option is presented as a boolean control.
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    let ultracode = config["configOptions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|option| option["id"] == "ultracode")
        .expect("ultracode option");
    assert_eq!(ultracode["type"], "boolean");
    assert_eq!(ultracode["currentValue"], false);
}

#[test]
fn a_refused_starting_mode_fails_the_session_start() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let batch = new_session(&mut runtime, frame(derived("newSessionInAuto")));
    let id = request_id(&batch.egress()[0]);
    runtime
        .resolve_egress(batch.egress()[0].token().clone(), EgressResolution::Written)
        .unwrap();
    let refused = runtime.ingest(
        &frame(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Invalid params"}})),
        &Mapper,
    );
    assert!(matches!(
        refused,
        Err(AdapterRuntimeError::ConfigMutationRejected)
    ));
    assert!(runtime.session_config().is_none());
}

#[test]
fn a_session_already_in_sages_mode_is_ready_at_once_and_a_catalog_without_it_fails_closed() {
    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let mut manual = derived("newSessionInAuto");
    manual["result"]["modes"]["currentModeId"] = json!("default");
    let batch = new_session(&mut runtime, frame(manual));
    assert!(matches!(batch.events(), [SharedEvent::SessionReady { .. }]));
    assert!(batch.egress().is_empty());

    let mut runtime = ClaudeRuntime::fixture();
    initialize_0101(&mut runtime);
    let mut without = derived("newSessionInAuto");
    without["result"]["modes"]["availableModes"] = json!([
        {"id":"auto","name":"Auto"}
    ]);
    without["result"]["configOptions"] = json!([]);
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .unwrap();
    without["id"] = json!(request_id(&egress));
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(matches!(
        runtime.ingest(&frame(without), &Mapper),
        Err(AdapterRuntimeError::ConfigMutationRejected)
    ));
}

fn activation_intent(
    mode: ActivationMode,
    ceiling: crate::acp::intent::CapabilitySnapshotV1,
) -> ActivationIntent {
    ActivationIntent {
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
fn activation_writes_the_starting_mode_and_commits_ready_only_after_it_is_accepted() {
    use super::resume_regression_tests::{Journal, Sink};
    let mut probe = ClaudeRuntime::fixture();
    initialize_0101(&mut probe);
    let ceiling = probe.capabilities().unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let intent = activation_intent(
            ActivationMode::New {
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
        let mut session = derived("newSessionInAuto");
        session["id"] = json!(2);
        pump.push_stdout_chunk(&line(session), 2).await.unwrap();
        assert!(
            pump.try_into_idle_parts().is_err(),
            "not ready before the mode is set"
        );
    });
}

#[test]
fn activation_hands_off_once_the_starting_mode_is_accepted() {
    use super::resume_regression_tests::{Journal, Sink};
    let mut probe = ClaudeRuntime::fixture();
    initialize_0101(&mut probe);
    let ceiling = probe.capabilities().unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let intent = activation_intent(
            ActivationMode::New {
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
        let mut session = derived("newSessionInAuto");
        session["id"] = json!(2);
        pump.push_stdout_chunk(&line(session), 2).await.unwrap();
        pump.flush_one(&mut Sink).unwrap();
        let mut accepted = derived("setModeAccepted");
        accepted["id"] = json!(3);
        pump.push_stdout_chunk(&line(accepted), 3).await.unwrap();
        let parts = pump
            .try_into_idle_parts()
            .ok()
            .expect("ready after the starting mode");
        let (runtime, _, _, _) = parts.into_parts();
        assert_eq!(current_mode(&runtime), "default");
    });
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

fn fork_0101(runtime: &mut ClaudeRuntime) -> ExternalId {
    let egress = runtime.fork_session().unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&answer("forkLive", id), &Mapper).unwrap();
    let fork = ExternalId::new("externalSessionId", FORK.to_owned()).unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Forked(fork.clone()))
    );
    // The live fork announces its commands; they are not the parent's.
    let batch = runtime
        .ingest(&frame(derived("forkCommands")), &Mapper)
        .unwrap();
    assert!(matches!(batch.events(), [SharedEvent::Absorbed]));
    fork
}

#[test]
fn a_committed_live_fork_is_closed_in_the_parent_runtime() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_0101(&mut runtime);
    let fork = fork_0101(&mut runtime);
    let close = runtime
        .release_fork(&fork, true)
        .unwrap()
        .expect("a close frame");
    let request = payload(&close);
    assert_eq!(request["method"], "session/close");
    assert_eq!(request["params"], json!({"sessionId": FORK}));
    let id = request_id(&close);
    runtime
        .resolve_egress(close.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(runtime.native_rpc_pending());
    runtime.ingest(&answer("closeAnswer", id), &Mapper).unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    assert!(!runtime.native_rpc_pending());
    // The parent session is untouched.
    assert!(runtime.session_config().is_some());
}

#[test]
fn a_fork_committed_during_a_turn_is_closed_once_idle() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_0101(&mut runtime);
    let fork = fork_0101(&mut runtime);
    let (prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    assert!(runtime.defer_fork_release(&fork, true));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    assert!(runtime.deferred_fork_release().unwrap().is_none());
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"end_turn"}})),
            &Mapper,
        )
        .unwrap();
    let close = runtime.deferred_fork_release().unwrap().expect("deferred");
    let request = payload(&close);
    assert_eq!(request["method"], "session/close");
    assert_eq!(request["params"], json!({"sessionId": FORK}));
    let id = request_id(&close);
    runtime
        .resolve_egress(close.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&answer("closeAnswer", id), &Mapper).unwrap();
    assert_eq!(runtime.take_native_rpc_reply(), None);
    assert!(runtime.deferred_fork_release().unwrap().is_none());
}

#[test]
fn a_committed_fork_is_not_closed_when_the_adapter_does_not_advertise_close() {
    let mut runtime = ClaudeRuntime::fixture();
    let egress = runtime.command(AdapterCommand::Initialize).unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    let mut advertisement = derived("initialize");
    advertisement["id"] = json!(id);
    advertisement["result"]["agentCapabilities"]["sessionCapabilities"]
        .as_object_mut()
        .unwrap()
        .remove("close");
    runtime.ingest(&frame(advertisement), &Mapper).unwrap();
    let batch = new_session(
        &mut runtime,
        frame(json!({"jsonrpc":"2.0","id":0,"result":{"sessionId":"claude-session-1"}})),
    );
    assert!(matches!(batch.events(), [SharedEvent::SessionReady { .. }]));
    let fork = fork_0101(&mut runtime);
    assert!(runtime.release_fork(&fork, true).unwrap().is_none());
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
}

/// Opens permission request 7 in a running Turn and returns its Sage id.
fn open_permission(runtime: &mut ClaudeRuntime) -> (u64, SageServerRequestId) {
    let (prompt_id, token) = prompt(runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":"tool-1",
                    "kind":"edit","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(json!({
                "jsonrpc":"2.0","id":7,"method":"session/request_permission","params":{
                    "sessionId":"claude-session-1",
                    "toolCall":{"toolCallId":"tool-1","title":"Write file","kind":"edit",
                        "status":"pending","rawInput":{}},
                    "options":[
                        {"optionId":"allow","name":"Allow","kind":"allow_once"},
                        {"optionId":"reject","name":"Reject","kind":"reject_once"}
                    ]
                }
            })),
            &Mapper,
        )
        .unwrap();
    let Some(SharedEvent::PermissionRequest {
        external_request_id,
        ..
    }) = batch.events().last()
    else {
        panic!("permission card")
    };
    (
        prompt_id,
        SageServerRequestId::parse(external_request_id.as_str()).unwrap(),
    )
}

#[test]
fn a_permission_the_adapter_stops_waiting_for_is_settled_and_answered_cancelled() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_0101(&mut runtime);
    let (prompt_id, request) = open_permission(&mut runtime);
    let cancel = runtime.command(AdapterCommand::Cancel).unwrap();
    runtime
        .resolve_egress(cancel.into_token(), EgressResolution::Written)
        .unwrap();

    let batch = runtime
        .ingest(&frame(derived("cancelPermissionRequest")), &Mapper)
        .unwrap();
    let [
        SharedEvent::PermissionResolved {
            external_request_id,
            state,
            ..
        },
    ] = batch.events()
    else {
        panic!("the open card is settled: {:?}", batch.events().len())
    };
    assert_eq!(external_request_id.as_str(), request.as_str());
    assert_eq!(*state, ServerRequestState::Cancelled);
    // SDK 1.x keeps waiting for this answer; SDK 2.x drops it quietly.
    let [reply] = batch.egress() else {
        panic!("one cancelled answer")
    };
    assert_eq!(
        payload(reply),
        json!({"jsonrpc":"2.0","id":7,"result":{"outcome":{"outcome":"cancelled"}}})
    );
    let token = reply.token().clone();
    runtime.commit_mandatory_egress(&token).unwrap();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    // The card can no longer be answered by the user.
    assert!(runtime.cancel_permission(&request).is_err());
    // A repeated notice is late, and the cancelled prompt ends the Turn.
    let again = runtime
        .ingest(&frame(derived("cancelPermissionRequest")), &Mapper)
        .unwrap();
    assert!(matches!(again.events(), [SharedEvent::IgnoredLate]));
    let end = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":prompt_id,"result":{"stopReason":"cancelled"}})),
            &Mapper,
        )
        .unwrap();
    assert!(end.events().iter().any(|event| matches!(
        event,
        SharedEvent::Turn {
            state: TurnState::Cancelled,
            ..
        }
    )));
}

#[test]
fn a_permission_sage_already_cancelled_is_not_settled_twice() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_0101(&mut runtime);
    let (_, request) = open_permission(&mut runtime);
    let reply = runtime.cancel_permission(&request).unwrap();
    let token = reply.into_token();
    runtime.commit_mandatory_egress(&token).unwrap();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    let batch = runtime
        .ingest(&frame(derived("cancelPermissionRequest")), &Mapper)
        .unwrap();
    assert!(matches!(batch.events(), [SharedEvent::IgnoredLate]));
    assert!(batch.egress().is_empty());
}

#[test]
fn a_cancel_request_for_no_permission_stays_an_unsupported_notice() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_0101(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","method":"$/cancel_request","params":{"requestId":99}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(), [SharedEvent::Unsupported(_)]));
}
