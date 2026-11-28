//! The shared host's router over `codex app-server` 0.167.1 frame shapes, as
//! observed with two concurrent threads on one process.

use serde_json::{Value, json};

use super::CodexScope;
use crate::acp::runtime::shared_host::{
    HostRouter, LeaseSlot, MAX_LEASES, Outbound, Routed, RouterError,
};

const A: &str = "01a22123-6d24-7cb0-8e30-c93153c0261e";
const B: &str = "01a22123-71c4-7192-ab92-0456e7e8155d";

fn written(outbound: Result<Outbound, RouterError>) -> Value {
    match outbound {
        Ok(Outbound::Write(frame)) => frame,
        other => panic!("expected a write, got {other:?}"),
    }
}

fn only(routed: Routed) -> (LeaseSlot, Value) {
    assert!(routed.reply.is_none());
    let [delivery] = <[_; 1]>::try_from(routed.deliveries).expect("one delivery");
    delivery
}

fn thread_started(thread: &str) -> Value {
    json!({"result": {"thread": {"id": thread, "cwd": "C:\\work"}, "approvalPolicy": "never"}})
}

/// Two leases each start a thread with the same JSON-RPC id 2.
fn two_threads() -> (HostRouter<CodexScope>, LeaseSlot, LeaseSlot) {
    let mut router = HostRouter::new(CodexScope);
    router.set_handshake(json!({"userAgent": "codex/0.167.1"}));
    let a = router.admit().unwrap();
    let b = router.admit().unwrap();
    for (slot, thread) in [(a, A), (b, B)] {
        let start = json!({"id": 2, "method": "thread/start", "params": {"cwd": thread}});
        let host = written(router.outbound(slot, start));
        let mut reply = thread_started(thread);
        reply["id"] = host["id"].clone();
        let (to, frame) = only(router.inbound(reply).unwrap());
        assert_eq!((to, frame["id"].clone()), (slot, json!(2)));
    }
    (router, a, b)
}

#[test]
fn the_same_lease_id_from_two_chats_gets_distinct_host_ids_and_comes_back_to_its_chat() {
    let (mut router, a, b) = two_threads();
    let turn =
        |thread: &str| json!({"id": 7, "method": "turn/start", "params": {"threadId": thread}});
    let host_a = written(router.outbound(a, turn(A)))["id"].clone();
    let host_b = written(router.outbound(b, turn(B)))["id"].clone();
    assert_ne!(host_a, host_b);

    // The process answers B first.
    let (to, frame) = only(router.inbound(json!({"id": host_b, "result": {}})).unwrap());
    assert_eq!((to, frame["id"].clone()), (b, json!(7)));
    let (to, frame) = only(router.inbound(json!({"id": host_a, "result": {}})).unwrap());
    assert_eq!((to, frame["id"].clone()), (a, json!(7)));
    // A repeated answer has no request left to match.
    assert_eq!(
        router.inbound(json!({"id": host_a, "result": {}})).unwrap(),
        Routed::default()
    );
}

#[test]
fn interleaved_thread_frames_reach_only_their_chat_and_process_frames_reach_both() {
    let (mut router, a, b) = two_threads();
    let delta = |thread: &str| json!({"method": "item/agentMessage/delta", "params": {"threadId": thread, "turnId": "t", "delta": "x"}});
    assert_eq!(only(router.inbound(delta(B)).unwrap()).0, b);
    assert_eq!(only(router.inbound(delta(A)).unwrap()).0, a);
    let started = json!({"method": "thread/started", "params": {"thread": {"id": A}}});
    assert_eq!(only(router.inbound(started).unwrap()).0, a);

    for method in [
        "account/rateLimits/updated",
        "account/updated",
        "configWarning",
    ] {
        let routed = router
            .inbound(json!({"method": method, "params": {}}))
            .unwrap();
        let slots: Vec<_> = routed.deliveries.iter().map(|(slot, _)| *slot).collect();
        assert_eq!(slots, vec![a, b], "{method}");
    }
    // A frame of a thread no chat owns is not anyone's.
    assert_eq!(router.inbound(delta("other")).unwrap(), Routed::default());
}

#[test]
fn an_approval_goes_to_its_chat_and_only_that_chat_may_answer_it() {
    let (mut router, a, b) = two_threads();
    let approval = json!({
        "id": 0, "method": "item/commandExecution/requestApproval",
        "params": {"kind": "command", "threadId": A, "turnId": "t", "itemId": "exec-1", "cwd": "C:\\a"}
    });
    assert_eq!(only(router.inbound(approval).unwrap()).0, a);
    let answer = json!({"id": 0, "result": {"decision": "decline"}});
    assert_eq!(
        router.outbound(b, answer.clone()),
        Err(RouterError::ForeignAnswer)
    );
    assert_eq!(written(router.outbound(a, answer.clone())), answer);
    assert_eq!(router.outbound(a, answer), Err(RouterError::ForeignAnswer));
}

#[test]
fn a_server_request_for_a_thread_no_chat_owns_is_refused_so_the_process_never_waits() {
    let (mut router, _, _) = two_threads();
    let elicit = json!({"id": 4, "method": "mcpServer/elicitation/request",
        "params": {"threadId": "gone", "turnId": "t", "serverName": "spike"}});
    let routed = router.inbound(elicit).unwrap();
    assert!(routed.deliveries.is_empty());
    let reply = routed.reply.expect("a refusal");
    assert_eq!(reply["id"], json!(4));
    assert!(reply["error"]["code"].is_i64());
}

#[test]
fn each_chat_handshake_is_answered_by_the_host_and_never_reaches_the_process() {
    let mut router = HostRouter::new(CodexScope);
    let slot = router.admit().unwrap();
    let initialize = json!({"id": 1, "method": "initialize", "params": {"clientInfo": {}}});
    assert_eq!(
        router.outbound(slot, initialize.clone()),
        Err(RouterError::HandshakePending)
    );
    router.set_handshake(json!({"userAgent": "codex/0.167.1"}));
    assert_eq!(
        router.outbound(slot, initialize),
        Ok(Outbound::Answer(
            json!({"id": 1, "result": {"userAgent": "codex/0.167.1"}})
        ))
    );
    let initialized = json!({"method": "initialized"});
    assert_eq!(router.outbound(slot, initialized), Ok(Outbound::Drop));
}

#[test]
fn a_released_chat_gets_nothing_more_and_its_slot_serves_the_next_chat() {
    let (mut router, a, b) = two_threads();
    let pending = written(router.outbound(
        a,
        json!({"id": 9, "method": "thread/unsubscribe", "params": {"threadId": A}}),
    ));
    assert_eq!(router.release(a), Ok(vec![A.to_owned()]));
    assert_eq!(
        router
            .inbound(json!({"id": pending["id"], "result": {}}))
            .unwrap(),
        Routed::default()
    );
    let closed = json!({"method": "thread/closed", "params": {"threadId": A}});
    assert_eq!(router.inbound(closed).unwrap(), Routed::default());
    assert_eq!(router.release(a), Err(RouterError::UnknownLease));

    let next = router.admit().unwrap();
    assert_eq!(next, a);
    // Resuming the still-loaded thread gives it to the new chat.
    router.bind_thread(next, A).unwrap();
    assert_eq!(router.bind_thread(b, A), Err(RouterError::ThreadOwned));
}

#[test]
fn a_host_serves_at_most_four_chats() {
    let mut router = HostRouter::new(CodexScope);
    for _ in 0..MAX_LEASES {
        router.admit().unwrap();
    }
    assert_eq!(router.admit(), Err(RouterError::Full));
    assert_eq!(router.leases(), MAX_LEASES);
}

#[test]
fn a_subagent_thread_follows_its_parent_chat_until_codex_closes_it() {
    let (mut router, a, _) = two_threads();
    let child = "01a22200-0000-7000-8000-00000000c41d";
    let activity = json!({"method": "item/started", "params": {"threadId": A, "turnId": "t",
        "item": {"type": "subAgentActivity", "id": "sa-1", "agentThreadId": child}}});
    assert_eq!(only(router.inbound(activity).unwrap()).0, a);
    let child_delta =
        json!({"method": "item/agentMessage/delta", "params": {"threadId": child, "turnId": "c"}});
    assert_eq!(only(router.inbound(child_delta.clone()).unwrap()).0, a);

    let closed = json!({"method": "thread/closed", "params": {"threadId": child}});
    assert_eq!(only(router.inbound(closed).unwrap()).0, a);
    assert_eq!(router.inbound(child_delta).unwrap(), Routed::default());
    assert_eq!(router.owner(A), Some(a));
}

#[test]
fn a_chat_with_too_many_requests_waiting_is_refused_instead_of_queued() {
    let (mut router, a, b) = two_threads();
    let read = |id: u64| json!({"id": id, "method": "thread/read", "params": {"threadId": A}});
    let mut accepted = 0;
    while router.outbound(a, read(100 + accepted)).is_ok() {
        accepted += 1;
    }
    assert_eq!(router.outbound(a, read(1)), Err(RouterError::PendingFull));
    assert_eq!(accepted, 64);
    // The other chat is not starved by it.
    written(router.outbound(
        b,
        json!({"id": 1, "method": "thread/read", "params": {"threadId": B}}),
    ));
}
