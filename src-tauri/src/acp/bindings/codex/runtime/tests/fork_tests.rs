//! `thread/fork` replay against the pinned Codex 0.146 generated schema.

use super::*;
use crate::acp::goal::NativeRpcReply;

fn pinned_fork_schema() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../../../../testdata/acp/codex/observed-0146-fork-schema.json"
    ))
    .unwrap()
}

fn fork_request(runtime: &mut CodexRuntime) -> (u64, Value) {
    let egress = runtime.fork_session().expect("fork request");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .expect("fork written");
    (id, request)
}

fn forked_thread(id: u64, thread: Value) -> Frame {
    frame(json!({"id":id,"result":{
        "thread": thread,
        "model":"gpt-6-astra","modelProvider":"openai","cwd":"C:\\repo",
        "approvalPolicy":"never","approvalsReviewer":"user",
        "sandbox":{"type":"readOnly"}
    }}))
}

#[test]
fn the_fork_request_is_the_pinned_0146_thread_fork_shape() {
    let schema = pinned_fork_schema();
    assert_eq!(schema["codexRuntimeVersion"], "0.146.0");
    assert_eq!(schema["clientRequestDeclaresMethod"], true);
    let effort: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../testdata/acp/codex/observed-0146-effort-schema.json"
    ))
    .unwrap();
    // Both pins come from the same generated schema of the same entrypoint.
    assert_eq!(schema["entrypointSha256"], effort["entrypointSha256"]);
    assert_eq!(
        schema["schemaFiles"]["ClientRequest.json"],
        effort["schemaFiles"]["ClientRequest.json"]
    );
    assert!(
        effort["clientRequestMethods"]
            .as_array()
            .unwrap()
            .contains(&json!("thread/fork"))
    );

    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    assert!(runtime.capabilities().unwrap().native_session_fork);
    let (_, request) = fork_request(&mut runtime);
    assert_eq!(request["method"], schema["method"]);
    let params = request["params"].as_object().unwrap();
    let admitted = schema["params"]["properties"].as_array().unwrap();
    for key in params.keys() {
        assert!(
            admitted.contains(&json!(key)),
            "{key} is not in the pinned schema"
        );
    }
    for required in schema["params"]["required"].as_array().unwrap() {
        assert!(params.contains_key(required.as_str().unwrap()));
    }
    assert_eq!(params["threadId"], "thread-1");
    // Sage forks only between Turns, so it never names an inclusive anchor.
    assert!(!params.contains_key("lastTurnId"));
}

#[test]
fn a_fork_response_replays_into_a_typed_native_session() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let (id, _) = fork_request(&mut runtime);
    assert!(runtime.native_rpc_pending());
    assert!(
        runtime.child_read_pending(),
        "the forked Turns may exceed a frame, so oversize stays recoverable"
    );
    let batch = runtime
        .ingest(
            &forked_thread(
                id,
                json!({"id":"thread-fork","forkedFromId":"thread-1","turns":[
                    {"id":"turn-1","status":"completed","items":[]}
                ]}),
            ),
            &Mapper,
        )
        .expect("fork response");
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(batch.egress().is_empty());
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Forked(
            ExternalId::new("externalSessionId", "thread-fork".to_owned()).unwrap()
        ))
    );
    assert!(!runtime.native_rpc_pending());
    // The parent stays bound to its own thread and can prompt again.
    running_turn(&mut runtime);
}

#[test]
fn an_error_a_foreign_lineage_or_a_self_fork_is_rejected() {
    for answer in [
        json!({"error":{"code":-32600,"message":"turn in progress"}}),
        json!({"result":{"thread":{"id":"thread-fork","forkedFromId":"thread-9"}}}),
        json!({"result":{"thread":{"id":"thread-1","forkedFromId":"thread-1"}}}),
        json!({"result":{"thread":{"forkedFromId":"thread-1"}}}),
    ] {
        let mut runtime = fixture_runtime();
        ready(&mut runtime);
        let (id, _) = fork_request(&mut runtime);
        let mut answer = answer;
        answer["id"] = json!(id);
        runtime.ingest(&frame(answer), &Mapper).expect("settled");
        assert_eq!(
            runtime.take_native_rpc_reply(),
            Some(NativeRpcReply::Rejected)
        );
        assert!(!runtime.native_rpc_pending());
    }
}

#[test]
fn an_oversize_or_unbounded_fork_answer_is_too_large_and_settles_once() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let (id, _) = fork_request(&mut runtime);
    runtime
        .accept_oversized_response(id)
        .expect("oversize fork");
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::TooLarge)
    );
    assert!(!runtime.native_rpc_pending());

    let (id, _) = fork_request(&mut runtime);
    let unbounded = forked_thread(
        id,
        json!({"id":"thread-fork","forkedFromId":"thread-1","turns":vec![json!({}); 600]}),
    );
    runtime
        .discard_invalid_response(&unbounded)
        .expect("structurally unbounded fork");
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::TooLarge)
    );
    assert!(!runtime.native_rpc_pending());
}

#[test]
fn a_fork_is_refused_before_negotiation_during_a_turn_and_while_one_is_pending() {
    let mut unnegotiated = fixture_runtime();
    assert!(unnegotiated.fork_session().is_err());

    let mut running = fixture_runtime();
    ready(&mut running);
    running_turn(&mut running);
    assert!(running.fork_session().is_err());

    let mut pending = fixture_runtime();
    ready(&mut pending);
    let _ = fork_request(&mut pending);
    assert!(pending.fork_session().is_err());
}

const CAPTURED_PARENT: &str = "01a16b24-2c2c-7cc0-8f64-09d61ef125e6";
const CAPTURED_FORK: &str = "01a16b24-3b97-7dc3-8cc6-bf79c587a204";

/// The live codex 0.146 app-server frames recorded around a `thread/fork`
/// with no prompt sent (`harness-traces/codex/fork-notifications`), with the
/// captured parent thread renamed to the fixture's bound `thread-1`.
fn captured_fork_frames() -> Vec<(String, Value)> {
    include_str!(
        "../../../../../../tests/fixtures/harness-traces/codex/fork-notifications/frames.jsonl"
    )
    .lines()
    .filter(|line| !line.trim().is_empty())
    .map(|line| {
        let record: Value =
            serde_json::from_str(&line.replace(CAPTURED_PARENT, "thread-1")).unwrap();
        (
            record["direction"].as_str().unwrap().to_owned(),
            record["frame"].clone(),
        )
    })
    .collect()
}

/// Replays every frame Codex sent after `thread/fork` and before the first
/// `thread/unsubscribe`, answering the runtime's own fork request id.
fn replay_captured_fork(runtime: &mut CodexRuntime) -> Vec<AdapterEvent> {
    let frames = captured_fork_frames();
    let fork_at = frames
        .iter()
        .position(|(direction, frame)| direction == "out" && frame["method"] == "thread/fork")
        .expect("captured fork request");
    let captured_id = frames[fork_at].1["id"].clone();
    let (id, _) = fork_request(runtime);
    let mut events = Vec::new();
    for (direction, frame) in &frames[fork_at + 1..] {
        if direction == "out" {
            assert_eq!(frame["method"], "thread/unsubscribe");
            break;
        }
        let mut frame = frame.clone();
        if frame.get("method").is_none() && frame["id"] == captured_id {
            frame["id"] = json!(id);
        }
        let batch = runtime
            .ingest(&super::frame(frame.clone()), &Mapper)
            .unwrap_or_else(|error| panic!("captured post-fork frame {frame} failed: {error:?}"));
        events.extend(batch.events().iter().cloned());
        assert!(batch.egress().is_empty());
    }
    events
}

#[test]
fn the_captured_post_fork_frames_leave_the_parent_runtime_bound_and_idle() {
    let frames = captured_fork_frames();
    // The live capture: Codex announces the fork thread both before it
    // answers `thread/fork` (MCP startup) and after it (`thread/started`).
    let fork_at = frames
        .iter()
        .position(|(_, frame)| frame["method"] == "thread/fork")
        .unwrap();
    let announced = |method: &str| {
        frames[fork_at..].iter().any(|(direction, frame)| {
            direction == "in"
                && frame["method"] == method
                && (frame["params"]["threadId"] == CAPTURED_FORK
                    || frame["params"]["thread"]["id"] == CAPTURED_FORK)
        })
    };
    assert!(announced("thread/started"));
    assert!(announced("mcpServer/startupStatus/updated"));

    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let events = replay_captured_fork(&mut runtime);
    // Every event is one the idle pump admits (`completion.rs`), so the
    // parent runtime survives a fork taken between Turns.
    assert!(events.iter().all(|event| matches!(
        event,
        AdapterEvent::IgnoredLate | AdapterEvent::IgnoredDuplicate | AdapterEvent::Absorbed
    )));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Forked(
            ExternalId::new("externalSessionId", CAPTURED_FORK.to_owned()).unwrap()
        ))
    );
    // Late fork-thread frames stay absorbed and the parent can prompt again.
    let late = runtime
        .ingest(
            &frame(json!({"method":"thread/status/changed","params":{
                "threadId":CAPTURED_FORK,"status":{"type":"systemError"}
            }})),
            &Mapper,
        )
        .expect("late fork frame");
    assert!(matches!(late.events(), [AdapterEvent::IgnoredLate]));
    running_turn(&mut runtime);
}

#[test]
fn a_fork_thread_is_released_with_the_captured_unsubscribe_and_never_the_parent() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    replay_captured_fork(&mut runtime);
    runtime.take_native_rpc_reply();
    let fork = ExternalId::new("externalSessionId", CAPTURED_FORK.to_owned()).unwrap();
    let parent = ExternalId::new("externalSessionId", "thread-1".to_owned()).unwrap();
    assert!(runtime.release_fork(&parent, true).is_err());

    let egress = runtime
        .release_fork(&fork, true)
        .expect("release")
        .expect("an unsubscribe frame");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["method"], "thread/unsubscribe");
    assert_eq!(request["params"], json!({"threadId": CAPTURED_FORK}));
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(runtime.native_rpc_pending());
    let mut answer = captured_fork_frames()
        .into_iter()
        .find(|(direction, frame)| direction == "in" && frame["result"]["status"] == "unsubscribed")
        .expect("captured unsubscribe answer")
        .1;
    answer["id"] = json!(id);
    let batch = runtime.ingest(&frame(answer), &Mapper).expect("released");
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    assert!(!runtime.native_rpc_pending());
    // Released once; a second release has nothing to let go of.
    assert!(runtime.release_fork(&fork, true).is_err());
    running_turn(&mut runtime);
}

#[test]
fn unreleased_forks_are_capped_per_parent_runtime() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    for index in 0..crate::acp::bindings::codex::state::MAX_UNRELEASED_FORKS {
        let (id, _) = fork_request(&mut runtime);
        runtime
            .ingest(
                &forked_thread(
                    id,
                    json!({"id":format!("thread-fork-{index}"),"forkedFromId":"thread-1"}),
                ),
                &Mapper,
            )
            .expect("fork");
        runtime.take_native_rpc_reply();
    }
    assert!(runtime.fork_session().is_err(), "the ledger is full");
    let first = ExternalId::new("externalSessionId", "thread-fork-0".to_owned()).unwrap();
    let egress = runtime.release_fork(&first, false).unwrap().unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{"status":"unsubscribed"}})),
            &Mapper,
        )
        .unwrap();
    assert!(
        runtime.fork_session().is_ok(),
        "a released slot admits a fork"
    );
}

#[test]
fn a_release_refused_during_a_turn_is_written_once_the_runtime_is_idle() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    replay_captured_fork(&mut runtime);
    runtime.take_native_rpc_reply();
    let fork = ExternalId::new("externalSessionId", CAPTURED_FORK.to_owned()).unwrap();
    assert!(runtime.deferred_fork_release().unwrap().is_none());
    running_turn(&mut runtime);
    // The refused fork's release meets a running Turn: it is deferred and
    // the caller is answered at once.
    assert!(runtime.defer_fork_release(&fork, false));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    assert!(runtime.deferred_fork_release().unwrap().is_none());
    runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"completed","items":[]}
            }})),
            &Mapper,
        )
        .expect("turn completes");
    // Idle again: the deferred release is written as background work.
    let egress = runtime.deferred_fork_release().unwrap().expect("deferred");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["method"], "thread/delete");
    assert_eq!(request["params"], json!({"threadId": CAPTURED_FORK}));
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(
        !runtime.native_rpc_pending(),
        "a background release never holds the RPC slot of a user command"
    );
    // The captured thread/delete answer (local evidence raw-codex-delete).
    runtime
        .ingest(&frame(json!({"id":id,"result":{}})), &Mapper)
        .expect("released");
    assert_eq!(runtime.take_native_rpc_reply(), None);
    assert!(runtime.deferred_fork_release().unwrap().is_none());
    assert!(runtime.release_fork(&fork, true).is_err(), "released once");
    // Codex then announces the deleted thread; it stays absorbed.
    for late in [
        json!({"method":"thread/status/changed","params":{"threadId":CAPTURED_FORK,"status":{"type":"notLoaded"}}}),
        json!({"method":"thread/deleted","params":{"threadId":CAPTURED_FORK}}),
    ] {
        let batch = runtime.ingest(&frame(late), &Mapper).expect("absorbed");
        assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    }
}

#[test]
fn a_committed_fork_unsubscribes_and_a_full_ledger_is_a_capacity_refusal() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    for index in 0..crate::acp::bindings::codex::state::MAX_UNRELEASED_FORKS {
        let (id, _) = fork_request(&mut runtime);
        runtime
            .ingest(
                &forked_thread(
                    id,
                    json!({"id":format!("thread-fork-{index}"),"forkedFromId":"thread-1"}),
                ),
                &Mapper,
            )
            .unwrap();
        runtime.take_native_rpc_reply();
    }
    assert!(matches!(
        runtime.fork_session(),
        Err(AdapterRuntimeError::BatchCount)
    ));
    let first = ExternalId::new("externalSessionId", "thread-fork-0".to_owned()).unwrap();
    let egress = runtime.release_fork(&first, true).unwrap().unwrap();
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["method"], "thread/unsubscribe");
}

fn forks(runtime: &mut CodexRuntime, count: usize) -> Vec<ExternalId> {
    (0..count)
        .map(|index| {
            let (id, _) = fork_request(runtime);
            runtime
                .ingest(
                    &forked_thread(
                        id,
                        json!({"id":format!("thread-fork-{index}"),"forkedFromId":"thread-1"}),
                    ),
                    &Mapper,
                )
                .expect("fork");
            runtime.take_native_rpc_reply();
            ExternalId::new("externalSessionId", format!("thread-fork-{index}")).unwrap()
        })
        .collect()
}

fn complete_turn(runtime: &mut CodexRuntime) {
    runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"completed","items":[]}
            }})),
            &Mapper,
        )
        .expect("turn completes");
}

fn written_background(runtime: &mut CodexRuntime) -> (u64, Value) {
    let egress = runtime
        .deferred_fork_release()
        .unwrap()
        .expect("a deferred release");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    (id, request)
}

fn refuse(runtime: &mut CodexRuntime, id: u64) {
    runtime
        .ingest(
            &frame(json!({"id":id,"error":{"code":-32600,"message":"busy"}})),
            &Mapper,
        )
        .expect("refused");
}

#[test]
fn a_foreground_release_that_meets_a_background_release_is_wanted_and_answered() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let forks = forks(&mut runtime, 2);
    running_turn(&mut runtime);
    assert!(runtime.defer_fork_release(&forks[0], true));
    runtime.take_native_rpc_reply();
    complete_turn(&mut runtime);
    let (background, _) = written_background(&mut runtime);
    // The second fork's commit lands while the first release is in flight.
    assert!(
        runtime
            .release_fork(&forks[1], true)
            .expect("answered")
            .is_none(),
        "no frame while a background release holds the release slot"
    );
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    runtime
        .ingest(&frame(json!({"id":background,"result":{}})), &Mapper)
        .expect("first released");
    let (_, request) = written_background(&mut runtime);
    assert_eq!(request["method"], "thread/unsubscribe");
    assert_eq!(request["params"], json!({"threadId": "thread-fork-1"}));
}

#[test]
fn a_refused_background_release_is_retried_a_bounded_number_of_times() {
    use crate::acp::bindings::codex::state::{MAX_FORK_RELEASE_ATTEMPTS, MAX_UNRELEASED_FORKS};
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let forks = forks(&mut runtime, MAX_UNRELEASED_FORKS);
    assert!(runtime.fork_session().is_err(), "the ledger is full");
    running_turn(&mut runtime);
    assert!(runtime.defer_fork_release(&forks[0], false));
    runtime.take_native_rpc_reply();
    complete_turn(&mut runtime);
    for _ in 0..MAX_FORK_RELEASE_ATTEMPTS {
        let (id, request) = written_background(&mut runtime);
        assert_eq!(request["method"], "thread/delete");
        refuse(&mut runtime, id);
        assert_eq!(runtime.take_native_rpc_reply(), None);
    }
    // Bounded: the thread is given up and no longer holds a ledger slot, so
    // the full-ledger refusal (let its current turn finish) stays true.
    assert!(runtime.deferred_fork_release().unwrap().is_none());
    assert!(runtime.release_fork(&forks[0], false).is_err());
    assert!(
        runtime.fork_session().is_ok(),
        "the given-up slot admits a fork"
    );
}

#[test]
fn a_foreground_refusal_is_retried_when_the_runtime_is_idle() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let forks = forks(&mut runtime, 1);
    let egress = runtime.release_fork(&forks[0], true).unwrap().unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    refuse(&mut runtime, id);
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );
    let (_, request) = written_background(&mut runtime);
    assert_eq!(request["method"], "thread/unsubscribe");
}

#[test]
fn the_bound_thread_is_never_deferred_or_written_as_a_fork_release() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let bound = ExternalId::new("externalSessionId", "thread-1".to_owned()).unwrap();
    runtime.inner.remember_fork(bound.clone());
    running_turn(&mut runtime);
    assert!(!runtime.defer_fork_release(&bound, false));
    complete_turn(&mut runtime);
    assert!(runtime.deferred_fork_release().unwrap().is_none());
}
