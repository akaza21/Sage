//! 0.101.0 starts a fork live in the parent runtime; a committed fork is closed there.

use super::*;

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
        frame(
            json!({"jsonrpc":"2.0","id":0,"result":{"sessionId":"claude-session-1","modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"}]}}}),
        ),
    );
    assert!(matches!(batch.events(), [SharedEvent::SessionReady { .. }]));
    let fork = fork_0101(&mut runtime);
    assert!(runtime.release_fork(&fork, true).unwrap().is_none());
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Released)
    );
}
