use super::*;
use crate::acp::goal::NativeRpcReply;
use sage_acp_protocol::{contract::SageTurnId, session_control::SessionControl};

const TURN: &str = "turn-6f9619ff-8b86-d011-b42d-00cf4fc964ff";

fn earlier_target() -> sage_acp_protocol::session_control::ConversationTarget {
    sage_acp_protocol::session_control::ConversationTarget {
        turn_id: SageTurnId::parse(TURN).unwrap(),
        native_turn_id: Some(ExternalId::new("nativeTurnId", "turn-1").unwrap()),
        following_messages: 1,
        prompt_digest: None,
    }
}

#[test]
fn an_earlier_correlated_turn_is_reverted_after_later_turns_finish() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    run_turn(&mut runtime, TURN, "turn-1");
    run_turn(
        &mut runtime,
        "turn-00000000-0000-4000-8000-000000000002",
        "turn-2",
    );
    let control = SessionControl::RewindConversation {
        before: earlier_target(),
    };
    let egress = runtime.session_control(&control).unwrap();
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(request["method"], "thread/revert");
    assert_eq!(
        request["params"],
        json!({"threadId":"thread-1","beforeTurnId":"turn-1"})
    );
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request["id"],"result":{"thread":{"id":"thread-1","turns":[]}}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
    assert!(runtime.answered_control(&control));
}

#[test]
fn a_delivered_prompt_with_a_saved_digest_resolves_from_the_native_suffix() {
    use sha2::{Digest, Sha256};
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let mut target = earlier_target();
    target.native_turn_id = None;
    target.prompt_digest = Some(Sha256::digest(b"Inspect only").into());
    let control = SessionControl::RewindConversation { before: target };
    let egress = runtime.session_control(&control).unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    let batch=runtime.ingest(&frame(json!({"id":id,"result":{"data":[
        {"id":"turn-2","status":"completed","itemsView":"full","items":[{"type":"userMessage","content":[{"type":"text","text":"Later question"}]}]},
        {"id":"turn-1","status":"completed","itemsView":"full","items":[{"type":"userMessage","content":[{"type":"text","text":"Inspect only"}]}]}
    ],"nextCursor":null}})),&Mapper).unwrap();
    let (_, mut egress) = batch.into_parts();
    let request: Value = serde_json::from_slice(egress.pop().unwrap().frame().payload()).unwrap();
    assert_eq!(
        request["params"],
        json!({"threadId":"thread-1","beforeTurnId":"turn-1"})
    );
}

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
