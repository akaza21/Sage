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
    assert!(routed.replies.is_empty() && routed.fault.is_none());
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
        "remoteControl/status/changed",
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
        json!({"id": 9, "method": "thread/read", "params": {"threadId": A}}),
    ));
    assert_eq!(router.release(a), Ok((vec![A.to_owned()], Vec::new())));
    assert_eq!(
        router
            .inbound(json!({"id": pending["id"], "result": {}}))
            .unwrap(),
        Routed::default()
    );
    let closed = json!({"method": "thread/closed", "params": {"threadId": A}});
    assert_eq!(router.inbound(closed).unwrap(), Routed::default());
    assert_eq!(router.release(a), Err(RouterError::UnknownLease));

    // The next chat reuses the slot; the released chat cannot act as it.
    let next = router.admit().unwrap();
    assert_ne!(next, a);
    assert_eq!(router.bind_thread(next, A), Ok(()));
    assert_eq!(router.bind_thread(b, A), Err(RouterError::ThreadOwned));
    only(router.inbound(approval(30, A)).unwrap());
    let answer = json!({"id": 30, "result": {"decision": "decline"}});
    assert_eq!(
        router.outbound(a, answer.clone()),
        Err(RouterError::UnknownLease)
    );
    assert_eq!(router.release(a), Err(RouterError::UnknownLease));
    assert_eq!(written(router.outbound(next, answer.clone())), answer);
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

fn approval(id: u64, thread: &str) -> Value {
    json!({"id": id, "method": "item/commandExecution/requestApproval",
        "params": {"kind": "command", "threadId": thread, "turnId": "t", "itemId": "exec-1"}})
}

fn refused_ids(routed: &Routed) -> Vec<Value> {
    routed
        .replies
        .iter()
        .map(|reply| reply["id"].clone())
        .collect()
}

#[test]
fn reading_another_chats_thread_answers_the_reader_without_moving_the_thread() {
    let (mut router, a, b) = two_threads();
    let read = json!({"id": 3, "method": "thread/read", "params": {"threadId": A}});
    let host = written(router.outbound(b, read))["id"].clone();
    let mut reply = thread_started(A);
    reply["id"] = host;
    let (to, frame) = only(router.inbound(reply).unwrap());
    assert_eq!((to, frame["id"].clone()), (b, json!(3)));
    assert_eq!(router.owner(A), Some(a));
}

#[test]
fn a_resume_of_a_thread_another_chat_holds_is_still_answered_and_reported() {
    let (mut router, a, b) = two_threads();
    let resume = json!({"id": 4, "method": "thread/resume", "params": {"threadId": A}});
    let host = written(router.outbound(b, resume))["id"].clone();
    let mut reply = thread_started(A);
    reply["id"] = host;
    let routed = router.inbound(reply).unwrap();
    assert_eq!(routed.deliveries.len(), 1);
    assert_eq!(routed.deliveries[0].0, b);
    assert_eq!(routed.deliveries[0].1["id"], json!(4));
    assert_eq!(routed.fault, Some((b, RouterError::ThreadOwned)));
    assert_eq!(router.owner(A), Some(a));
}

#[test]
fn an_error_response_comes_back_to_its_chat_under_the_chats_own_id() {
    let (mut router, _, b) = two_threads();
    let resume = json!({"id": "r-1", "method": "thread/resume", "params": {"threadId": "lost"}});
    let host = written(router.outbound(b, resume))["id"].clone();
    let error = json!({"id": host, "error": {"code": -32600, "message": "no rollout found for thread id lost"}});
    let (to, frame) = only(router.inbound(error).unwrap());
    assert_eq!((to, frame["id"].clone()), (b, json!("r-1")));
    assert_eq!(router.owner("lost"), None);
}

#[test]
fn closing_a_chat_refuses_the_approvals_it_never_answered() {
    let (mut router, a, _) = two_threads();
    only(router.inbound(approval(11, A)).unwrap());
    only(router.inbound(approval(12, A)).unwrap());
    let (threads, refusals) = router.release(a).unwrap();
    assert_eq!(threads, vec![A.to_owned()]);
    let ids: Vec<_> = refusals.iter().map(|reply| reply["id"].clone()).collect();
    assert_eq!(ids, vec![json!(11), json!(12)]);
    // Codex keeps the released thread loaded for a while; its next approval
    // is refused at once rather than held.
    assert_eq!(
        refused_ids(&router.inbound(approval(13, A)).unwrap()),
        vec![json!(13)]
    );
}

#[test]
fn an_approval_over_a_chats_bound_is_refused_rather_than_left_waiting() {
    let (mut router, a, _) = two_threads();
    for id in 0..64 {
        assert_eq!(only(router.inbound(approval(id, A)).unwrap()).0, a);
    }
    let routed = router.inbound(approval(64, A)).unwrap();
    assert!(routed.deliveries.is_empty());
    assert_eq!(refused_ids(&routed), vec![json!(64)]);
}

#[test]
fn a_server_request_naming_no_thread_is_refused_and_shown_to_no_chat() {
    let (mut router, _, _) = two_threads();
    let routed = router
        .inbound(json!({"id": 4242, "method": "attestation/generate", "params": {}}))
        .unwrap();
    assert!(routed.deliveries.is_empty());
    assert_eq!(refused_ids(&routed), vec![json!(4242)]);
}

#[test]
fn answers_to_the_hosts_own_requests_go_to_the_host_not_a_chat() {
    let (mut router, _, _) = two_threads();
    let id = router.host_request_id().unwrap();
    let routed = router
        .inbound(json!({"id": id, "result": {"account": null}}))
        .unwrap();
    assert!(routed.deliveries.is_empty());
    assert_eq!(
        routed.host,
        Some(json!({"id": id, "result": {"account": null}}))
    );
}

#[test]
fn a_server_request_for_a_thread_no_chat_owns_is_refused_at_once() {
    let (mut router, _, _) = two_threads();
    let child = "01a22200-0000-7000-8000-0000000000ff";
    let routed = router.inbound(approval(21, child)).unwrap();
    assert!(routed.deliveries.is_empty());
    assert_eq!(refused_ids(&routed), vec![json!(21)]);
}

#[test]
fn what_the_router_would_have_to_keep_is_refused_when_oversized() {
    let (mut router, a, _) = two_threads();
    let long_thread = "t".repeat(513);
    assert_eq!(
        refused_ids(&router.inbound(approval(50, &long_thread)).unwrap()),
        vec![json!(50)]
    );
    assert_eq!(
        router.bind_thread(a, &long_thread),
        Err(RouterError::Malformed)
    );
    let mut long_id = approval(0, A);
    long_id["id"] = json!("i".repeat(200));
    let routed = router.inbound(long_id).unwrap();
    assert!(routed.deliveries.is_empty());
    assert_eq!(refused_ids(&routed), vec![json!("i".repeat(200))]);
}

#[test]
fn a_request_codex_resolves_itself_stops_counting_against_the_chat() {
    let (mut router, a, _) = two_threads();
    let resolved = |id: u64| json!({"method": "serverRequest/resolved", "params": {"threadId": A, "requestId": id}});
    for id in 0..100 {
        assert_eq!(only(router.inbound(approval(id, A)).unwrap()).0, a);
        assert_eq!(only(router.inbound(resolved(id)).unwrap()).0, a);
    }
    // The chat may no longer answer a resolved request, and closing it owes
    // Codex nothing for them.
    let late = json!({"id": 99, "result": {"decision": "decline"}});
    assert_eq!(router.outbound(a, late), Err(RouterError::ForeignAnswer));
    only(router.inbound(approval(100, A)).unwrap());
    let (_, refusals) = router.release(a).unwrap();
    let ids: Vec<_> = refusals.iter().map(|reply| reply["id"].clone()).collect();
    assert_eq!(ids, vec![json!(100)]);
}

#[test]
fn a_resolution_after_its_thread_closed_still_retires_the_request() {
    let (mut router, a, _) = two_threads();
    only(router.inbound(approval(60, A)).unwrap());
    let closed = json!({"method": "thread/closed", "params": {"threadId": A}});
    only(router.inbound(closed).unwrap());
    let resolved =
        json!({"method": "serverRequest/resolved", "params": {"threadId": A, "requestId": 60}});
    assert_eq!(router.inbound(resolved).unwrap(), Routed::default());
    assert_eq!(router.release(a), Ok((Vec::new(), Vec::new())));
}

#[test]
fn a_fork_the_parent_chat_unsubscribes_can_be_resumed_by_another_chat() {
    let (mut router, a, b) = two_threads();
    let fork = "01a22300-0000-7000-8000-00000000f0c1";
    let host = written(router.outbound(
        a,
        json!({"id": 5, "method": "thread/fork", "params": {"threadId": A}}),
    ))["id"]
        .clone();
    let mut reply = thread_started(fork);
    reply["id"] = host;
    only(router.inbound(reply).unwrap());
    assert_eq!(router.owner(fork), Some(a));
    written(router.outbound(
        a,
        json!({"id": 6, "method": "thread/unsubscribe", "params": {"threadId": fork}}),
    ));
    assert_eq!(router.owner(fork), None);
    let host = written(router.outbound(
        b,
        json!({"id": 7, "method": "thread/resume", "params": {"threadId": fork}}),
    ))["id"]
        .clone();
    let mut reply = thread_started(fork);
    reply["id"] = host;
    only(router.inbound(reply).unwrap());
    assert_eq!(router.owner(fork), Some(b));
    assert_eq!(router.owner(A), Some(a));
}

#[test]
fn a_chat_request_id_the_router_would_have_to_keep_unbounded_is_refused() {
    let (mut router, a, _) = two_threads();
    let read = |id: Value| json!({"id": id, "method": "thread/read", "params": {"threadId": A}});
    assert_eq!(
        router.outbound(a, read(json!("i".repeat(129)))),
        Err(RouterError::Malformed)
    );
    assert_eq!(
        router.outbound(a, read(json!({"nested": 1}))),
        Err(RouterError::Malformed)
    );
    written(router.outbound(a, read(json!("i".repeat(128)))));
    written(router.outbound(a, read(json!(u64::MAX))));
}
