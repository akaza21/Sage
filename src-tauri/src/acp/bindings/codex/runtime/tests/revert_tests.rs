use super::*;
use crate::acp::goal::NativeRpcReply;
use sage_acp_protocol::{contract::SageTurnId, session_control::SessionControl};

const TURN: &str = "turn-6f9619ff-8b86-d011-b42d-00cf4fc964ff";

fn undo(turn: &str) -> SessionControl {
    SessionControl::UndoTurn {
        turn_id: SageTurnId::parse(turn.to_owned()).unwrap(),
    }
}

fn prompt_command(turn: &str) -> AdapterCommand {
    AdapterCommand::Prompt {
        prompt: Prompt::new("prompt", "Inspect only").expect("prompt"),
        client_message_id: RequiredText::new("client", turn).expect("client"),
        access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
    }
}

fn finished_turn(runtime: &mut CodexRuntime) {
    run_turn(runtime, TURN, "turn-1");
}

fn run_turn(runtime: &mut CodexRuntime, sage: &str, codex: &str) {
    let egress = runtime.command(prompt_command(sage)).expect("prompt");
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    for frame_value in [
        json!({"id":id,"result":{"turn":{"id":codex}}}),
        json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":codex}}}),
    ] {
        runtime.ingest(&frame(frame_value), &Mapper).expect("turn");
    }
    assert!(runtime.session_control(&undo(sage)).is_err());
    runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":codex,"status":"completed","items":[]}
            }})),
            &Mapper,
        )
        .expect("turn completes");
}

fn written_undo(runtime: &mut CodexRuntime) -> (u64, Value) {
    let egress = runtime.session_control(&undo(TURN)).expect("undo");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    (id, request)
}

#[test]
fn undo_reverts_the_thread_before_the_latest_turn_once() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    finished_turn(&mut runtime);
    assert!(
        runtime
            .session_control(&undo("turn-00000000-0000-4000-8000-000000000000"))
            .is_err()
    );
    let (id, request) = written_undo(&mut runtime);
    assert_eq!(request["method"], "thread/revert");
    assert_eq!(
        request["params"],
        json!({"threadId":"thread-1","beforeTurnId":"turn-1"})
    );
    assert!(runtime.native_rpc_pending());
    assert!(
        runtime.session_control(&undo(TURN)).is_err(),
        "one at a time"
    );
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{"thread":{"id":"thread-1","turns":[]}}})),
            &Mapper,
        )
        .expect("reverted");
    assert!(!runtime.native_rpc_pending());
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
    assert!(
        runtime.session_control(&undo(TURN)).is_err(),
        "the Turn is already gone"
    );
    assert!(!runtime.answered_control(&undo("turn-00000000-0000-4000-8000-000000000000")));
    assert!(runtime.answered_control(&undo(TURN)));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
}

#[test]
fn a_refused_undo_carries_codex_reason_and_may_be_tried_again() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    finished_turn(&mut runtime);
    let (id, _) = written_undo(&mut runtime);
    runtime
        .ingest(
            &frame(json!({"id":id,"error":{"code":-32600,"message":"thread is not paginated"}})),
            &Mapper,
        )
        .expect("refused");
    let Some(NativeRpcReply::ControlRefused(Some(reason))) = runtime.take_native_rpc_reply() else {
        panic!("refusal with its reason");
    };
    assert_eq!(reason.as_str(), "thread is not paginated");
    written_undo(&mut runtime);
}

const SECOND: &str = "turn-00000000-0000-4000-8000-000000000002";
const THIRD: &str = "turn-00000000-0000-4000-8000-000000000003";

/// A rewind as the store resolves it: a stored native id, or none for a
/// turn Sage has only a prompt digest for.
fn rewind(turn: &str, native: Option<&str>) -> SessionControl {
    SessionControl::RewindConversation {
        before: sage_acp_protocol::session_control::ConversationTarget {
            turn_id: SageTurnId::parse(turn.to_owned()).unwrap(),
            native_turn_id: native.map(|id| ExternalId::new("nativeTurnId", id).unwrap()),
            following_messages: 0,
            prompt_digest: native.is_none().then_some([7; 32]),
        },
    }
}

fn three_turns(runtime: &mut CodexRuntime) {
    finished_turn(runtime);
    run_turn(runtime, SECOND, "turn-2");
    run_turn(runtime, THIRD, "turn-3");
}

#[test]
fn rewinding_at_a_middle_turn_reverts_before_its_native_id_and_retires_the_last_turn_undo() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    three_turns(&mut runtime);
    assert!(runtime.answered_control(&rewind(SECOND, None)));
    let Some(NativeRpcReply::ControlRefused(Some(reason))) = runtime.take_native_rpc_reply() else {
        panic!("a target without a native id is refused before any write");
    };
    assert_eq!(reason.as_str(), crate::acp::store::rewind::OLDER_MESSAGES);

    let cut = rewind(SECOND, Some("turn-2"));
    assert!(!runtime.answered_control(&cut));
    let egress = runtime.session_control(&cut).expect("rewind");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(
        request,
        json!({"id":id,"method":"thread/revert",
            "params":{"threadId":"thread-1","beforeTurnId":"turn-2"}})
    );
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(
        runtime.session_control(&undo(THIRD)).is_err(),
        "one at a time"
    );
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{"thread":{"id":"thread-1","turns":[]}}})),
            &Mapper,
        )
        .expect("reverted");
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
    assert!(
        runtime.session_control(&undo(THIRD)).is_err(),
        "the cut removed the latest Turn"
    );
}

#[test]
fn a_rewind_never_written_may_be_retried() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    three_turns(&mut runtime);
    let cut = rewind(SECOND, Some("turn-2"));
    let egress = runtime.session_control(&cut).expect("rewind");
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::QueueRejected)
        .unwrap();
    assert!(!runtime.native_rpc_pending());
    let retry = runtime.session_control(&cut).expect("retry admitted");
    let id = request_id(&retry);
    runtime
        .resolve_egress(retry.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":id,"error":{"code":-32600,"message":"unknown turn"}})),
            &Mapper,
        )
        .expect("refused");
    assert!(matches!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::ControlRefused(Some(_)))
    ));
    let egress = runtime.session_control(&undo(THIRD)).expect("undo kept");
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["params"]["beforeTurnId"], "turn-3");
}

fn fork_at(turn: &str, native: Option<&str>) -> SessionControl {
    let SessionControl::RewindConversation { before } = rewind(turn, native) else {
        unreachable!()
    };
    SessionControl::ForkAt { target: before }
}

#[test]
fn forking_at_a_middle_turn_cuts_before_its_native_id_and_deletes_an_uncommitted_fork() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    three_turns(&mut runtime);
    assert!(
        runtime.session_control(&fork_at(SECOND, None)).is_err(),
        "a prompt position is never a Codex Turn"
    );
    let cut = fork_at(SECOND, Some("turn-2"));
    let unwritten = runtime.session_control(&cut).expect("fork");
    runtime
        .resolve_egress(unwritten.into_token(), EgressResolution::QueueRejected)
        .unwrap();
    assert!(!runtime.native_rpc_pending());
    let egress = runtime.session_control(&cut).expect("retry admitted");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(
        request,
        json!({"id":id,"method":"thread/fork","params":{
            "threadId":"thread-1","beforeTurnId":"turn-2","excludeTurns":true}})
    );
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    // The live 0.167.1 answer to this request, with the fixture's ids and
    // paths: a new thread whose one Turn keeps turn 1's native id.
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{
                "thread":{"id":"thread-fork","sessionId":"thread-fork",
                    "forkedFromId":"thread-1","parentThreadId":null,"preview":"",
                    "ephemeral":false,"historyMode":"paginated","status":{"type":"idle"},
                    "modelProvider":"openai","model":"gpt-6.1-sol","cwd":"C:\\repo",
                    "cliVersion":"0.167.1","turns":[]},
                "model":"gpt-6.1-sol","modelProvider":"openai","cwd":"C:\\repo",
                "approvalPolicy":"never","approvalsReviewer":"user",
                "sandbox":{"type":"readOnly","networkAccess":false}
            }})),
            &Mapper,
        )
        .expect("forked");
    let fork = ExternalId::new("externalSessionId", "thread-fork".to_owned()).unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Forked(fork.clone()))
    );
    // The fork Chat's commit failed: the fork thread is deleted, never the
    // source, which keeps its own latest-turn undo.
    let release = runtime.release_fork(&fork, false).unwrap().unwrap();
    let request: Value = serde_json::from_slice(release.frame().payload()).unwrap();
    assert_eq!(request["method"], "thread/delete");
    assert_eq!(request["params"], json!({"threadId":"thread-fork"}));
    let id = request_id(&release);
    runtime
        .resolve_egress(release.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(&frame(json!({"id":id,"result":{}})), &Mapper)
        .expect("deleted");
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
    let egress = runtime.session_control(&undo(THIRD)).expect("undo kept");
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["params"]["beforeTurnId"], "turn-3");
}

#[test]
fn stopping_an_activity_is_not_a_codex_control() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let stop = SessionControl::StopActivity {
        activity_id: ExternalId::new("activityId", "task-1".to_owned()).unwrap(),
    };
    assert!(runtime.session_control(&stop).is_err());
}

#[test]
fn a_turn_codex_refused_to_start_is_not_undone() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let egress = runtime.command(prompt_command(TURN)).expect("prompt");
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":id,"error":{"code":-32600,"message":"busy"}})),
            &Mapper,
        )
        .expect("refused start");
    assert!(runtime.session_control(&undo(TURN)).is_err());
}

#[test]
fn a_turn_sent_during_an_undo_runs_and_can_itself_be_undone() {
    const NEXT: &str = "turn-00000000-0000-4000-8000-000000000001";
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    finished_turn(&mut runtime);
    let (id, _) = written_undo(&mut runtime);
    run_turn(&mut runtime, NEXT, "turn-2");
    runtime
        .ingest(
            &frame(json!({"id":id,"result":{"thread":{"id":"thread-1","turns":[]}}})),
            &Mapper,
        )
        .expect("reverted");
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
    let egress = runtime.session_control(&undo(NEXT)).expect("undo next");
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["params"]["beforeTurnId"], "turn-2");
}
