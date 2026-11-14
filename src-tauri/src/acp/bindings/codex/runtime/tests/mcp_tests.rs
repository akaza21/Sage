use super::*;
use crate::acp::{
    goal::NativeRpcReply,
    mcp::inventory::{McpAuth, McpConnection},
};
use sage_acp_protocol::session_control::SessionControl;

fn sign_in(name: &str) -> SessionControl {
    SessionControl::SignInMcpServer {
        name: RequiredText::new("mcpServerName", name).unwrap(),
    }
}

fn sent(runtime: &mut CodexRuntime, control: &SessionControl) -> (u64, Value) {
    let egress = runtime.session_control(control).expect("mcp request");
    let id = request_id(&egress);
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    (id, request)
}

fn answer(runtime: &mut CodexRuntime, id: u64, body: Value) {
    let mut reply = json!({"id": id});
    reply
        .as_object_mut()
        .unwrap()
        .extend(body.as_object().unwrap().clone());
    runtime.ingest(&frame(reply), &Mapper).expect("answer");
}

#[test]
fn the_list_reads_the_threads_servers_one_request_at_a_time() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let (id, request) = sent(&mut runtime, &SessionControl::ReadMcpServers);
    assert_eq!(request["method"], "mcpServerStatus/list");
    assert_eq!(
        request["params"],
        json!({"threadId":"thread-1","detail":"toolsAndAuthOnly","limit":64})
    );
    assert!(runtime.native_rpc_pending());
    assert!(
        runtime.session_control(&sign_in("notion")).is_err(),
        "one MCP request at a time"
    );
    answer(
        &mut runtime,
        id,
        json!({"result":{"data":[{
            "name":"notion","authStatus":"notLoggedIn","runtimeStatus":"authenticationRequired",
            "tools":{},"resources":[],"resourceTemplates":[]
        }],"nextCursor":null}}),
    );
    assert!(!runtime.native_rpc_pending());
    let Some(NativeRpcReply::McpServers(listed)) = runtime.take_native_rpc_reply() else {
        panic!("the inventory");
    };
    assert_eq!(listed.servers.len(), 1);
    assert_eq!(listed.servers[0].connection, McpConnection::SignInRequired);
    assert_eq!(listed.servers[0].auth, McpAuth::SignedOut);
}

#[test]
fn a_sign_in_answers_with_a_native_url_and_a_refusal_with_codexs_reason() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let (id, request) = sent(&mut runtime, &sign_in("notion"));
    assert_eq!(request["method"], "mcpServer/oauth/login");
    assert_eq!(
        request["params"],
        json!({"name":"notion","threadId":"thread-1"})
    );
    answer(
        &mut runtime,
        id,
        json!({"result":{"authorizationUrl":"https://auth.example.com/authorize?state=x","loginId":"l1"}}),
    );
    let Some(NativeRpcReply::McpSignIn(url)) = runtime.take_native_rpc_reply() else {
        panic!("a sign-in URL");
    };
    assert_eq!(url.as_str(), "https://auth.example.com/authorize?state=x");

    let (id, _) = sent(&mut runtime, &sign_in("notion"));
    answer(
        &mut runtime,
        id,
        json!({"result":{"authorizationUrl":"file:///c:/x.exe","loginId":"l2"}}),
    );
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );

    let (id, _) = sent(&mut runtime, &sign_in("files"));
    answer(
        &mut runtime,
        id,
        json!({"error":{"code":-32600,"message":"server files does not support OAuth"}}),
    );
    let Some(NativeRpcReply::ControlRefused(Some(reason))) = runtime.take_native_rpc_reply() else {
        panic!("a refusal with its reason");
    };
    assert_eq!(reason.as_str(), "server files does not support OAuth");
}

#[test]
fn a_reload_rereads_the_configuration() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let (id, request) = sent(&mut runtime, &SessionControl::ReloadMcpServers);
    assert_eq!(request["method"], "config/mcpServer/reload");
    assert_eq!(request["params"], Value::Null);
    answer(&mut runtime, id, json!({"result":{}}));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
}

#[test]
fn abandoned_lists_drop_their_late_answers_and_hold_only_further_lists() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let mut abandoned = Vec::new();
    for _ in 0..4 {
        abandoned.push(sent(&mut runtime, &SessionControl::ReadMcpServers).0);
        runtime.abandon_passive_read();
        assert!(!runtime.native_rpc_pending());
    }
    assert!(
        runtime
            .session_control(&SessionControl::ReadMcpServers)
            .is_err()
    );
    let (reload, _) = sent(&mut runtime, &SessionControl::ReloadMcpServers);
    answer(&mut runtime, reload, json!({"result":{}}));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
    answer(
        &mut runtime,
        abandoned[0],
        json!({"result":{"data":[],"nextCursor":null}}),
    );
    assert_eq!(runtime.take_native_rpc_reply(), None);
    let (id, _) = sent(&mut runtime, &SessionControl::ReadMcpServers);
    answer(
        &mut runtime,
        id,
        json!({"result":{"data":[],"nextCursor":null}}),
    );
    assert!(matches!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::McpServers(_))
    ));
}

#[test]
fn a_list_past_the_frame_bound_is_too_large_and_keeps_the_session() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    assert!(!runtime.child_read_pending());
    let (id, _) = sent(&mut runtime, &SessionControl::ReadMcpServers);
    assert!(
        runtime.child_read_pending(),
        "an oversized answer is recoverable"
    );
    runtime.accept_oversized_response(id).unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::TooLarge)
    );
    assert!(!runtime.native_rpc_pending());

    let (abandoned, _) = sent(&mut runtime, &SessionControl::ReadMcpServers);
    runtime.abandon_passive_read();
    assert!(runtime.child_read_pending());
    runtime.accept_oversized_response(abandoned).unwrap();
    assert_eq!(runtime.take_native_rpc_reply(), None);
    assert!(!runtime.child_read_pending());
    sent(&mut runtime, &SessionControl::ReadMcpServers);
}
