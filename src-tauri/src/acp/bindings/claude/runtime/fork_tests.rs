//! Unstable ACP `session/fork` replay for the characterised claude-agent-acp
//! 0.76.0 / SDK 1.4.0 pair, using the captured initialize advertisement. The
//! 0.101.0 / SDK 2.4.0 live fork is replayed in `adapter_0101_tests.rs`.

use serde_json::{Value, json};

use super::fork::MAX_TRACKED_FORKS;
use super::tests::{Mapper, frame, request_id};
use super::*;
use crate::acp::{
    bindings::adapter::{AdapterCommand, EgressResolution, HarnessAdapter},
    goal::NativeRpcReply,
    intent::Prompt,
};

const ROOT: &str = r"C:\fixture\root";

fn pinned() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../../../testdata/acp/claude/observed-0760-fork.json"
    ))
    .unwrap()
}

/// The `initialize` result recorded from claude-agent-acp 0.76.0.
fn captured_initialize_result() -> Value {
    include_str!("../../../../../tests/fixtures/harness-traces/claude-code/smoke/frames.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|record| record["direction"] == "in" && record["frame"]["id"] == 1)
        .expect("captured initialize response")["frame"]["result"]
        .clone()
}

fn ready_with(runtime: &mut ClaudeRuntime, result: Value) {
    let egress = runtime.command(AdapterCommand::Initialize).unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":result})),
            &Mapper,
        )
        .expect("initialize");
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: ROOT.into(),
            model: None,
        })
        .unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"claude-session-1"}})),
            &Mapper,
        )
        .expect("session");
}

fn fork(runtime: &mut ClaudeRuntime) -> (u64, Value) {
    let egress = runtime.fork_session().expect("fork request");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    (id, request)
}

#[test]
fn the_captured_0760_advertisement_negotiates_native_fork() {
    let result = captured_initialize_result();
    assert!(result["agentCapabilities"]["sessionCapabilities"]["fork"].is_object());
    let offer =
        super::super::ClaudeCapabilityOffer::from_initialize_value(&json!({"result":result}))
            .unwrap();
    assert!(offer.fork_session);
    assert!(
        super::super::negotiate_capabilities(&offer)
            .unwrap()
            .native_session_fork
    );
}

#[test]
fn the_fork_request_and_response_replay_the_pinned_session_fork_schema() {
    let pinned = pinned();
    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    assert!(runtime.capabilities().unwrap().native_session_fork);
    let (id, request) = fork(&mut runtime);
    assert_eq!(request["method"], pinned["method"]);
    let params = request["params"].as_object().unwrap();
    let admitted: Vec<&Value> = pinned["request"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .chain(pinned["request"]["optional"].as_array().unwrap())
        .collect();
    for key in params.keys() {
        assert!(
            admitted.contains(&&json!(key)),
            "{key} is not in the pinned schema"
        );
    }
    for required in pinned["request"]["required"].as_array().unwrap() {
        assert!(params.contains_key(required.as_str().unwrap()));
    }
    assert_eq!(params["sessionId"], "claude-session-1");
    assert_eq!(params["cwd"], ROOT);
    assert!(
        !params.contains_key("_meta"),
        "no vendor fork point is sent"
    );
    assert!(runtime.native_rpc_pending());

    let mut response = pinned["fixtureExchange"]["response"].clone();
    response["id"] = json!(id);
    let batch = runtime.ingest(&frame(response), &Mapper).unwrap();
    assert!(matches!(batch.events(), [SharedEvent::IgnoredLate]));
    let forked = pinned["fixtureExchange"]["response"]["result"]["sessionId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Forked(
            ExternalId::new("externalSessionId", forked).unwrap()
        ))
    );
    assert!(!runtime.native_rpc_pending());
}

#[test]
fn an_error_or_self_named_fork_is_rejected() {
    for answer in [
        json!({"error":{"code":-32602,"message":"Fork point message was not found"}}),
        json!({"result":{"sessionId":"claude-session-1"}}),
        json!({"result":{}}),
    ] {
        let mut runtime = ClaudeRuntime::fixture();
        ready_with(&mut runtime, captured_initialize_result());
        let (id, _) = fork(&mut runtime);
        let mut answer = answer;
        answer["jsonrpc"] = json!("2.0");
        answer["id"] = json!(id);
        runtime.ingest(&frame(answer), &Mapper).unwrap();
        assert_eq!(
            runtime.take_native_rpc_reply(),
            Some(NativeRpcReply::Rejected)
        );
    }
}

#[test]
fn fork_fails_closed_without_the_advertisement_and_during_a_prompt() {
    let mut unadvertised = ClaudeRuntime::fixture();
    ready_with(
        &mut unadvertised,
        json!({"protocolVersion":1,"agentCapabilities":{
            "sessionCapabilities":{"resume":{},"close":{}}
        }}),
    );
    assert!(!unadvertised.capabilities().unwrap().native_session_fork);
    assert!(matches!(
        unadvertised.fork_session(),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));

    let mut prompting = ClaudeRuntime::fixture();
    ready_with(&mut prompting, captured_initialize_result());
    prompting
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: RequiredText::new("client", "client-fork").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    assert!(prompting.fork_session().is_err());

    let mut pending = ClaudeRuntime::fixture();
    ready_with(&mut pending, captured_initialize_result());
    let _ = fork(&mut pending);
    assert!(pending.fork_session().is_err());
}

const CAPTURED_STORED: &str = "06511af0-f6d1-4ac4-9b4a-81ddbcb007e2";
const CAPTURED_FRESH: &str = "8c77ec7d-4e39-4f9b-872b-9cc71daa6eef";
const CAPTURED_FORK: &str = "a84469cf-bbc7-44cb-817f-63ec2725969d";

/// The live claude-agent-acp 0.76.0 frames around `session/fork` with no
/// prompt sent (`harness-traces/claude-code/fork`).
fn captured_fork_frames() -> Vec<(String, Value)> {
    include_str!("../../../../../tests/fixtures/harness-traces/claude-code/fork/frames.jsonl")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let record: Value = serde_json::from_str(line).unwrap();
            (
                record["direction"].as_str().unwrap().to_owned(),
                record["frame"].clone(),
            )
        })
        .collect()
}

fn captured_answer(frames: &[(String, Value)], method: &str, session: &str) -> Value {
    let id = frames
        .iter()
        .find(|(direction, frame)| {
            direction == "out"
                && frame["method"] == method
                && frame["params"]["sessionId"] == session
        })
        .expect("captured request")
        .1["id"]
        .clone();
    frames
        .iter()
        .find(|(direction, frame)| direction == "in" && frame["id"] == id)
        .expect("captured answer")
        .1
        .clone()
}

fn forked(runtime: &mut ClaudeRuntime, frames: &[(String, Value)]) -> ExternalId {
    let (id, _) = fork(runtime);
    let mut answer = captured_answer(frames, "session/fork", CAPTURED_STORED);
    answer["id"] = json!(id);
    let batch = runtime.ingest(&frame(answer), &Mapper).unwrap();
    assert!(matches!(batch.events(), [SharedEvent::IgnoredLate]));
    let fork = ExternalId::new("externalSessionId", CAPTURED_FORK.to_owned()).unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Forked(fork.clone()))
    );
    fork
}

#[test]
fn the_captured_fork_replies_replay_and_a_fresh_session_fork_is_rejected() {
    let frames = captured_fork_frames();
    // Nothing for the fork arrives between its answer and the next request:
    // 0.76.0 copies the stored transcript and keeps no live fork session.
    let fork_answer = frames
        .iter()
        .position(|(direction, frame)| {
            direction == "in" && frame["result"]["sessionId"] == CAPTURED_FORK
        })
        .unwrap();
    assert_eq!(frames[fork_answer + 1].0, "out");
    // So `session/close` cannot release it: the adapter does not know it.
    assert_eq!(
        captured_answer(&frames, "session/close", CAPTURED_FORK)["error"]["data"]["details"],
        "Session not found"
    );

    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    forked(&mut runtime, &frames);
    assert!(!runtime.native_rpc_pending());

    // A session with no stored prompt cannot be forked (the captured refusal).
    let mut fresh = ClaudeRuntime::fixture();
    ready_with(&mut fresh, captured_initialize_result());
    let (id, _) = fork(&mut fresh);
    let mut refusal = captured_answer(&frames, "session/fork", CAPTURED_FRESH);
    refusal["id"] = json!(id);
    fresh.ingest(&frame(refusal), &Mapper).unwrap();
    assert_eq!(
        fresh.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );
}

#[test]
fn updates_for_a_forked_session_are_absorbed_without_touching_the_parent() {
    let frames = captured_fork_frames();
    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    // While the fork is in flight, the fork id is not yet known.
    let (id, _) = fork(&mut runtime);
    let early = json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId": CAPTURED_FORK,
        "update":{"sessionUpdate":"available_commands_update","availableCommands":[]}
    }});
    let batch = runtime.ingest(&frame(early.clone()), &Mapper).unwrap();
    assert!(matches!(batch.events(), [SharedEvent::Absorbed]));
    let mut answer = captured_answer(&frames, "session/fork", CAPTURED_STORED);
    answer["id"] = json!(id);
    runtime.ingest(&frame(answer), &Mapper).unwrap();
    runtime.take_native_rpc_reply();
    // After it, every schema update kind for the fork id is absorbed too.
    for update in [
        json!({"sessionUpdate":"available_commands_update","availableCommands":[]}),
        json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"x"}}),
        json!({"sessionUpdate":"usage_update","used":1,"size":2}),
    ] {
        let value = json!({"jsonrpc":"2.0","method":"session/update","params":{
            "sessionId": CAPTURED_FORK, "update": update
        }});
        let batch = runtime.ingest(&frame(value), &Mapper).unwrap();
        assert!(matches!(batch.events(), [SharedEvent::Absorbed]));
    }
    // A session nobody forked is still foreign and refused.
    let foreign = json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId":"claude-session-9",
        "update":{"sessionUpdate":"available_commands_update","availableCommands":[]}
    }});
    assert!(runtime.ingest(&frame(foreign), &Mapper).is_err());
}

#[test]
fn a_committed_fork_is_closed_harmlessly_and_a_refused_fork_is_deleted() {
    let frames = captured_fork_frames();
    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    let fork = forked(&mut runtime, &frames);
    let parent = ExternalId::new("externalSessionId", "claude-session-1".to_owned()).unwrap();
    assert!(runtime.release_fork(&parent, false).is_err());

    // 0.76.0 advertises `session/close`, so a committed fork is closed in this
    // runtime; its fork is a stored copy, and the captured "Session not
    // found" answer still releases it.
    let close = runtime.release_fork(&fork, true).unwrap().expect("close");
    let request: Value = serde_json::from_slice(close.frame().payload()).unwrap();
    assert_eq!(request["method"], "session/close");
    assert_eq!(request["params"], json!({"sessionId": CAPTURED_FORK}));
    let id = request_id(&close);
    runtime
        .resolve_egress(close.into_token(), EgressResolution::Written)
        .unwrap();
    let mut answer = captured_answer(&frames, "session/close", CAPTURED_FORK);
    answer["id"] = json!(id);
    runtime.ingest(&frame(answer), &Mapper).unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );

    let egress = runtime.release_fork(&fork, false).unwrap().unwrap();
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["method"], "session/delete");
    assert_eq!(request["params"], json!({"sessionId": CAPTURED_FORK}));
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(runtime.native_rpc_pending());
    // `session/delete` answers `{}` (verified live against 0.76.0).
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    assert!(!runtime.native_rpc_pending());
}

#[test]
fn a_refused_fork_delete_during_a_prompt_is_written_once_idle() {
    let frames = captured_fork_frames();
    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    let fork = forked(&mut runtime, &frames);
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: RequiredText::new("client", "client-defer").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt_id = request_id(&prompt);
    runtime
        .resolve_egress(prompt.into_token(), EgressResolution::Written)
        .unwrap();
    // A refused fork's delete is deferred to the next idle moment.
    assert!(runtime.defer_fork_release(&fork, false));
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
    let egress = runtime.deferred_fork_release().unwrap().expect("deferred");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["method"], "session/delete");
    assert_eq!(request["params"], json!({"sessionId": CAPTURED_FORK}));
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(!runtime.native_rpc_pending());
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(runtime.take_native_rpc_reply(), None);
    assert!(runtime.deferred_fork_release().unwrap().is_none());
}

fn forked_session(runtime: &mut ClaudeRuntime, index: usize) -> ExternalId {
    let (id, _) = fork(runtime);
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{
                "sessionId": format!("claude-fork-{index}")
            }})),
            &Mapper,
        )
        .expect("forked");
    assert!(matches!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Forked(_))
    ));
    ExternalId::new("externalSessionId", format!("claude-fork-{index}")).unwrap()
}

/// Starts a prompt and returns its request id.
fn prompting(runtime: &mut ClaudeRuntime, client: &str) -> u64 {
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect only").unwrap(),
            client_message_id: RequiredText::new("client", client).unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let id = request_id(&prompt);
    runtime
        .resolve_egress(prompt.into_token(), EgressResolution::Written)
        .unwrap();
    id
}

fn end_prompt(runtime: &mut ClaudeRuntime, id: u64) {
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"end_turn"}})),
            &Mapper,
        )
        .unwrap();
}

#[test]
fn a_deferred_delete_that_is_never_written_stays_wanted() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    let fork = forked_session(&mut runtime, 0);
    let prompt = prompting(&mut runtime, "client-1");
    assert!(runtime.defer_fork_release(&fork, false));
    end_prompt(&mut runtime, prompt);
    let egress = runtime.deferred_fork_release().unwrap().expect("deferred");
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::WriteZeroBytes)
        .unwrap();
    let retry = runtime
        .deferred_fork_release()
        .unwrap()
        .expect("the unwritten delete is still wanted");
    let request: Value = serde_json::from_slice(retry.frame().payload()).unwrap();
    assert_eq!(request["method"], "session/delete");
    assert_eq!(request["params"], json!({"sessionId": "claude-fork-0"}));
}

#[test]
fn a_wanted_delete_is_never_evicted_and_a_full_ledger_refuses_new_forks() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    let forks: Vec<_> = (0..MAX_TRACKED_FORKS)
        .map(|index| forked_session(&mut runtime, index))
        .collect();
    // Every fork but the newest is refused during a prompt: its delete is wanted.
    let prompt = prompting(&mut runtime, "client-2");
    for fork in &forks[..MAX_TRACKED_FORKS - 1] {
        assert!(runtime.defer_fork_release(fork, false));
    }
    end_prompt(&mut runtime, prompt);
    // The one settled fork is evicted for a new fork; wanted deletes stay.
    let newest = forked_session(&mut runtime, MAX_TRACKED_FORKS);
    assert!(runtime.defer_fork_release(&forks[0], false));
    assert!(!runtime.defer_fork_release(&forks[MAX_TRACKED_FORKS - 1], false));
    // Now every slot is wanted, so a further fork is a capacity refusal.
    let prompt = prompting(&mut runtime, "client-3");
    assert!(runtime.defer_fork_release(&newest, false));
    end_prompt(&mut runtime, prompt);
    assert!(matches!(
        runtime.fork_session(),
        Err(AdapterRuntimeError::BatchCount)
    ));
    for _ in 0..MAX_TRACKED_FORKS {
        let egress = runtime.deferred_fork_release().unwrap().expect("wanted");
        let id = request_id(&egress);
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .unwrap();
        runtime
            .ingest(
                &frame(json!({"jsonrpc":"2.0","id":id,"result":{}})),
                &Mapper,
            )
            .unwrap();
    }
    assert!(runtime.deferred_fork_release().unwrap().is_none());
}

/// Writes every wanted delete at idle and returns the forks they name.
fn drain_wanted_deletes(runtime: &mut ClaudeRuntime) -> Vec<String> {
    let mut deleted = Vec::new();
    while let Some(egress) = runtime.deferred_fork_release().unwrap() {
        let id = request_id(&egress);
        let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
        assert_eq!(request["method"], "session/delete");
        deleted.push(request["params"]["sessionId"].as_str().unwrap().to_owned());
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .unwrap();
        runtime
            .ingest(
                &frame(json!({"jsonrpc":"2.0","id":id,"result":{}})),
                &Mapper,
            )
            .unwrap();
    }
    deleted
}

#[test]
fn a_fork_rejected_for_a_full_ledger_still_deletes_its_stored_copy() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    let forks: Vec<_> = (0..MAX_TRACKED_FORKS)
        .map(|index| forked_session(&mut runtime, index))
        .collect();
    // Every slot becomes wanted while the next fork is in flight.
    let (id, _) = fork(&mut runtime);
    for fork in &forks {
        assert!(runtime.defer_fork_release(fork, false));
    }
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"claude-fork-late"}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );
    // The adapter already stored the copy, so its delete is wanted too, and
    // the over-full ledger refuses further forks until the deletes land.
    assert!(matches!(
        runtime.fork_session(),
        Err(AdapterRuntimeError::BatchCount)
    ));
    let deleted = drain_wanted_deletes(&mut runtime);
    assert_eq!(deleted.len(), MAX_TRACKED_FORKS + 1);
    assert!(deleted.iter().any(|fork| fork == "claude-fork-late"));
    // Settled again, the ledger makes room for a new fork.
    forked_session(&mut runtime, MAX_TRACKED_FORKS + 1);
}

#[test]
fn an_unwritten_delete_is_given_up_after_three_attempts() {
    let mut runtime = ClaudeRuntime::fixture();
    ready_with(&mut runtime, captured_initialize_result());
    let fork = forked_session(&mut runtime, 0);
    let prompt = prompting(&mut runtime, "client-give-up");
    assert!(runtime.defer_fork_release(&fork, false));
    end_prompt(&mut runtime, prompt);
    for _ in 0..super::fork::MAX_FORK_DELETE_ATTEMPTS {
        let egress = runtime.deferred_fork_release().unwrap().expect("wanted");
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::WriteZeroBytes)
            .unwrap();
    }
    assert!(runtime.deferred_fork_release().unwrap().is_none());
}
