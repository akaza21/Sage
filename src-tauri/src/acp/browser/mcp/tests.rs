use super::*;
use crate::acp::browser::{BrowserRequest, BrowserResult, FakeBrowserCapability, NetworkEntry};
use serde_json::{Value, json};

#[test]
fn codex_approved_browser_mcp_call_still_requires_sage_permission() {
    use crate::acp::browser::permission::{
        BrowserAuthority, BrowserCapabilityDispatch, BrowserDispatchPermit,
        BrowserPermissionChoice, BrowserPermissionGate, BrowserPermissionPrompt,
        PendingBrowserResult,
    };
    use std::{cell::Cell, rc::Rc};

    let admission = crate::acp::mcp::McpAdmission::browser_stdio(
        r"C:\Sage\sage.exe".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            ("SAGE_BROWSER_MCP_SESSION".into(), "session-1".into()),
            (
                "SAGE_BROWSER_MCP_TOKEN".into(),
                "redacted-test-token".into(),
            ),
        ],
    )
    .unwrap();
    let codex = admission.codex_config().unwrap().unwrap();
    assert_eq!(
        codex["mcp_servers"]["sage-browser"]["default_tools_approval_mode"],
        "approve"
    );
    assert!(codex.get("default_tools_approval_mode").is_none());
    let unrelated = crate::acp::mcp::McpAdmission::for_test(vec![
        crate::acp::mcp::McpServerDescriptor::stdio_for_test(
            "unrelated",
            r"C:\Sage\other.exe",
            &[],
            vec![],
        ),
    ])
    .unwrap()
    .codex_config()
    .unwrap()
    .unwrap();
    assert!(
        unrelated["mcp_servers"]["unrelated"]
            .get("default_tools_approval_mode")
            .is_none()
    );

    struct Port;
    impl BrowserCapabilityDispatch for Port {
        fn dispatch(
            &mut self,
            _: &BrowserRequest,
            _: BrowserDispatchPermit,
        ) -> Result<PendingBrowserResult, BrowserFailure> {
            panic!("denied browser action must not reach the native port")
        }
    }
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    };
    let prompts = Rc::new(Cell::new(0));
    let seen = prompts.clone();
    let gate = BrowserPermissionGate::new(
        Port,
        move |prompt: &BrowserPermissionPrompt| {
            assert_eq!(prompt.operation, "snapshotPage");
            seen.set(seen.get() + 1);
            BrowserPermissionChoice::Deny
        },
        BrowserAuthority::attached(scope.clone(), "https://example.com/page", 1).unwrap(),
    );
    let (mut server, token) = BrowserMcpServer::issue(gate, scope, 1_000);
    server.connect(&token, "session-1", 1_001).unwrap();
    let reply = server
        .handle(&rpc(
            "tools/call",
            json!({"name":"snapshotPage","arguments":{}}),
        ))
        .unwrap();
    let reply: Value = serde_json::from_slice(&reply).unwrap();
    assert_eq!(prompts.get(), 1);
    assert_eq!(reply["result"]["isError"], true);
}

fn server() -> (BrowserMcpServer<FakeBrowserCapability>, BrowserMcpToken) {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    };
    let mut port = FakeBrowserCapability::default();
    port.open_tab(scope.clone());
    BrowserMcpServer::issue(port, scope, 1_000)
}

#[test]
fn open_tab_is_advertised_and_callable_without_an_attachment() {
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-1".into(),
        1_000,
    );
    server.connect(&token, "session-1", 1_001).unwrap();
    let listed: Value =
        serde_json::from_slice(&server.handle(&rpc("tools/list", json!({}))).unwrap()).unwrap();
    let tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "openTab")
        .unwrap();
    assert_eq!(tool["inputSchema"]["required"], json!(["url"]));
    assert!(
        tool["description"]
            .as_str()
            .unwrap()
            .contains("no tab is attached")
    );
    let opened: Value = serde_json::from_slice(
        &server
            .handle(&rpc(
                "tools/call",
                json!({"name":"openTab","arguments":{"url":"https://example.com/"}}),
            ))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(opened["result"]["isError"], false, "{opened}");
    let payload: Value =
        serde_json::from_str(opened["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(payload["result"]["tabId"], 1);
    let snapshot: Value = serde_json::from_slice(
        &server
            .handle(&rpc(
                "tools/call",
                json!({"name":"snapshotPage","arguments":{"scope":{"tabId":1}}}),
            ))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(snapshot["result"]["isError"], false, "{snapshot}");
}

#[test]
fn session_descriptor_is_inert_without_an_attached_tab() {
    struct NoTab;
    impl crate::acp::browser::BrowserCapability for NoTab {
        fn perform(
            &mut self,
            _: &BrowserRequest,
        ) -> Result<BrowserResult, crate::acp::browser::BrowserFailure> {
            Err(crate::acp::browser::BrowserFailure::NoAttachment)
        }
    }
    let (mut server, token) = BrowserMcpServer::issue_for_session(NoTab, "session-1".into(), 1_000);
    server.connect(&token, "session-1", 1_001).unwrap();
    let response = server.handle(&rpc("tools/call", json!({"_meta":{"progressToken":"probe-1"},"name":"snapshotPage","arguments":{"scope":{"sessionId":"session-1","tabId":7}}}))).unwrap();
    let value: Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(value["result"]["isError"], true);
    assert!(
        value["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("ask the user to attach one")
    );
}

#[test]
fn omitted_tab_id_uses_the_authenticated_attached_tab_or_fails_closed() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    for arguments in [json!({}), json!({"scope":{}})] {
        let response = server
            .handle(&rpc(
                "tools/call",
                json!({"name":"snapshotPage","arguments":arguments}),
            ))
            .unwrap();
        let response: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(response["result"]["isError"], false, "{response}");
    }
    struct NoTab;
    impl crate::acp::browser::BrowserCapability for NoTab {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            panic!("unattached request must not reach the port")
        }
    }
    let (mut unattached, token) =
        BrowserMcpServer::issue_for_session(NoTab, "session-1".into(), 1_000);
    unattached.connect(&token, "session-1", 1_001).unwrap();
    let response = unattached
        .handle(&rpc(
            "tools/call",
            json!({"name":"snapshotPage","arguments":{}}),
        ))
        .unwrap();
    let response: Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["result"]["isError"], true);
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("No browser tab attached")
    );
    assert!(
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("openTab")
    );
}

#[test]
fn session_descriptor_discovers_attached_tab_without_a_fixed_scope() {
    struct Attached(FakeBrowserCapability);
    impl crate::acp::browser::BrowserCapability for Attached {
        fn attached_tab_id(&self) -> Option<u64> {
            Some(7)
        }
        fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            self.0.perform(request)
        }
    }
    let mut fake = FakeBrowserCapability::default();
    fake.open_tab(BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    });
    let (mut server, token) =
        BrowserMcpServer::issue_for_session(Attached(fake), "session-1".into(), 1_000);
    server.connect(&token, "session-1", 1_001).unwrap();
    let response = server
        .handle(&rpc(
            "tools/call",
            json!({"name":"snapshotPage","arguments":{}}),
        ))
        .unwrap();
    let response: Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["result"]["isError"], false, "{response}");
}

#[test]
fn malformed_tool_call_returns_invalid_params_and_keeps_the_session() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    let invalid = server
        .handle(&rpc("tools/call", json!({"unexpected":true,"name":"snapshotPage","arguments":{"scope":{"sessionId":"session-1","tabId":7}}})))
        .unwrap();
    let value: Value = serde_json::from_slice(&invalid).unwrap();
    assert_eq!(value["id"], 1);
    assert_eq!(value["error"]["code"], -32602);
    let listed = server.handle(&rpc("tools/list", json!({}))).unwrap();
    let value: Value = serde_json::from_slice(&listed).unwrap();
    assert_eq!(value["result"]["tools"].as_array().unwrap().len(), 8);
}

#[test]
fn advertised_browser_tool_examples_reach_the_typed_handler() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    let scope = json!({"tabId":7});
    let handle = server
        .port
        .issue_handle(&BrowserScope {
            session_id: "session-1".into(),
            tab_id: 7,
        })
        .unwrap()
        .token()
        .to_owned();
    let listed = server.handle(&rpc("tools/list", json!({}))).unwrap();
    let listed: Value = serde_json::from_slice(&listed).unwrap();
    let act_tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "act")
        .unwrap();
    assert!(
        act_tool["description"]
            .as_str()
            .unwrap()
            .contains("action.kind")
    );
    let mut examples = vec![
        ("snapshotPage", json!({"scope":scope})),
        (
            "waitFor",
            json!({"scope":scope,"condition":{"kind":"text","text":"Example"},"timeoutMs":1}),
        ),
        ("screenshot", json!({"scope":scope,"region":null})),
        (
            "readConsole",
            json!({"scope":scope,"cursor":null,"limit":1}),
        ),
        (
            "readNetwork",
            json!({"scope":scope,"cursor":null,"limit":1}),
        ),
        (
            "navigate",
            json!({"scope":scope,"url":"https://example.com/"}),
        ),
    ];
    let action_examples = act_tool["inputSchema"]["properties"]["action"]["examples"]
        .as_array()
        .unwrap();
    assert_eq!(action_examples.len(), 4);
    for action in action_examples {
        let mut action = action.clone();
        action["handle"] = Value::String(handle.clone());
        examples.insert(1, ("act", json!({"scope":scope,"action":action})));
    }
    for (name, arguments) in examples {
        let response = match server.handle(&rpc(
            "tools/call",
            json!({"name":name,"arguments":arguments.clone()}),
        )) {
            Ok(response) => response,
            Err(BrowserMcpError::Unavailable) if name == "screenshot" => continue,
            Err(error) => panic!("{name} {arguments}: {error:?}"),
        };
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_ne!(
            value["error"]["code"], -32602,
            "{name} {arguments}: {value}"
        );
    }
}

#[test]
fn malformed_fill_names_action_kind_without_echoing_the_value() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    for action in [
        json!({"fill":"","handle":"missing","value":"do-not-echo"}),
        json!({"fill":{"kind":"fill","handle":"missing","value":"do-not-echo"}}),
    ] {
        let response = server
            .handle(&rpc(
                "tools/call",
                json!({"name":"act","arguments":{"scope":{"tabId":7},"action":action}}),
            ))
            .unwrap();
        let value: Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(value["error"]["code"], -32602);
        let message = value["error"]["message"].as_str().unwrap();
        assert!(message.contains("action.kind"), "{message}");
        assert!(!message.contains("do-not-echo"));
    }
}

#[test]
fn session_descriptor_accepts_first_connection_after_sixty_seconds() {
    let (mut server, token) = BrowserMcpServer::issue_for_session(
        FakeBrowserCapability::default(),
        "session-1".into(),
        1_000,
    );
    assert_eq!(server.connect(&token, "session-1", 62_001), Ok(()));
    assert_eq!(
        server.connect(&token, "session-1", 62_002),
        Err(BrowserMcpError::Unauthorized)
    );
}

#[test]
fn issued_browser_operation_with_unknown_outcome_is_not_reported_as_unavailable() {
    struct UnknownOutcome;
    impl crate::acp::browser::BrowserCapability for UnknownOutcome {
        fn perform(
            &mut self,
            _: &BrowserRequest,
        ) -> Result<BrowserResult, crate::acp::browser::BrowserFailure> {
            Err(crate::acp::browser::BrowserFailure::OutcomeUnknown)
        }
    }
    let (mut server, token) =
        BrowserMcpServer::issue_for_session(UnknownOutcome, "session-1".into(), 1_000);
    server.connect(&token, "session-1", 1_001).unwrap();
    let response = server.handle(&rpc("tools/call", json!({"name":"snapshotPage","arguments":{"scope":{"sessionId":"session-1","tabId":7}}}))).unwrap();
    let value: Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(value["result"]["isError"], true);
    let message = value["result"]["content"][0]["text"].as_str().unwrap();
    assert!(message.contains("outcome is unknown"));
    assert!(message.contains("Do not retry"));
}

fn rpc(method: &str, params: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})).unwrap()
}

#[test]
fn trusted_observer_sees_validated_browser_call_and_its_actual_outcome() {
    use std::sync::{Arc, Mutex};
    struct Recorder(Arc<Mutex<Vec<String>>>);
    impl BrowserOperationObserver for Recorder {
        fn started(&mut self, request: &BrowserRequest) -> Result<(), BrowserMcpError> {
            self.0.lock().unwrap().push(format!(
                "start:{}:{}",
                request.scope.session_id, request.scope.tab_id
            ));
            Ok(())
        }
        fn finished(
            &mut self,
            _: &BrowserRequest,
            outcome: &Result<
                crate::acp::browser::BrowserResponse,
                crate::acp::browser::BrowserFailure,
            >,
        ) -> Result<(), BrowserMcpError> {
            self.0
                .lock()
                .unwrap()
                .push(format!("end:{}", outcome.is_ok()));
            Ok(())
        }
    }
    let observed = Arc::new(Mutex::new(Vec::new()));
    let (mut server, token) = server();
    server.set_operation_observer(Box::new(Recorder(observed.clone())));
    server.connect(&token, "session-1", 1_001).unwrap();
    server
        .handle(&rpc(
            "tools/call",
            json!({"name":"snapshotPage","arguments":{"scope":{"tabId":7}}}),
        ))
        .unwrap();
    assert_eq!(*observed.lock().unwrap(), ["start:session-1:7", "end:true"]);
}

#[test]
fn observer_outcome_unknown_replies_on_same_id_and_keeps_mcp_session() {
    struct UnknownObserver;
    impl BrowserOperationObserver for UnknownObserver {
        fn started(&mut self, _: &BrowserRequest) -> Result<(), BrowserMcpError> {
            Ok(())
        }
        fn finished(
            &mut self,
            _: &BrowserRequest,
            _: &Result<crate::acp::browser::BrowserResponse, crate::acp::browser::BrowserFailure>,
        ) -> Result<(), BrowserMcpError> {
            Err(BrowserMcpError::OutcomeUnknown)
        }
    }
    let (mut server, token) = server();
    server.set_operation_observer(Box::new(UnknownObserver));
    server.connect(&token, "session-1", 1_001).unwrap();
    let response = server.handle(&rpc("tools/call", json!({"name":"snapshotPage","arguments":{"scope":{"sessionId":"session-1","tabId":7}}}))).unwrap();
    let value: Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(value["id"], 1);
    assert_eq!(value["result"]["isError"], true);
    assert!(
        value["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Do not retry")
    );
    assert!(server.handle(&rpc("tools/list", json!({}))).is_ok());
}

#[test]
fn tools_list_is_exactly_the_eight_typed_browser_commands() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    let bytes = server.handle(&rpc("tools/list", json!({}))).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let tools = value["result"]["tools"].as_array().unwrap();
    let names: Vec<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "openTab",
            "navigate",
            "snapshotPage",
            "act",
            "waitFor",
            "screenshot",
            "readConsole",
            "readNetwork"
        ]
    );
    for tool in tools {
        assert_eq!(tool["inputSchema"]["type"], "object");
        if tool["name"] != "openTab" {
            assert_eq!(
                tool["inputSchema"]["properties"]["scope"]["additionalProperties"],
                false
            );
            assert!(
                tool["inputSchema"]["properties"]["scope"]["description"]
                    .as_str()
                    .unwrap()
                    .contains("browser-N")
            );
            assert!(
                tool["inputSchema"]["properties"]["scope"]["properties"]
                    .get("sessionId")
                    .is_none()
            );
        }
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    }
    assert_eq!(
        tools[0]["inputSchema"]["properties"]["url"]["maxLength"],
        BROWSER_LIMITS.url_bytes
    );
    assert_eq!(tools[2]["inputSchema"]["required"], json!([]));
    assert_eq!(
        tools[3]["inputSchema"]["properties"]["action"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        tools[4]["inputSchema"]["properties"]["condition"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        tools[4]["inputSchema"]["properties"]["timeoutMs"]["maximum"],
        BROWSER_LIMITS.wait_timeout_ms
    );
    assert_eq!(
        tools[5]["inputSchema"]["properties"]["region"]["properties"]["width"]["maximum"],
        BROWSER_LIMITS.screenshot_width
    );
    for tool in &tools[6..] {
        assert_eq!(
            tool["inputSchema"]["properties"]["limit"]["maximum"],
            BROWSER_LIMITS.diagnostic_entries
        );
    }
}

#[test]
fn tools_list_accepts_standard_cursor_and_request_metadata() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    for params in [
        json!({"cursor":null}),
        json!({"_meta":{"progressToken":0}}),
        json!({"cursor":null,"_meta":{"progressToken":"probe-1"}}),
    ] {
        let bytes = server.handle(&rpc("tools/list", params)).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["result"]["tools"].as_array().map(Vec::len), Some(8));
    }
    let invalid = server
        .handle(&rpc("tools/list", json!({"unexpected":true})))
        .unwrap();
    let invalid: Value = serde_json::from_slice(&invalid).unwrap();
    assert_eq!(invalid["error"]["code"], -32602);
}

#[test]
fn unknown_tool_oversized_args_and_raw_cdp_are_rejected() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    let scope = json!({"sessionId":"session-1","tabId":7});
    let unknown = server
        .handle(&rpc(
            "tools/call",
            json!({"name":"Runtime.evaluate","arguments":{"scope":scope}}),
        ))
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&unknown).unwrap()["error"]["code"],
        -32602
    );
    let raw_cdp = server.handle(&rpc("tools/call", json!({"name":"navigate","arguments":{"scope":scope,"url":"https://example.com","method":"Runtime.evaluate"}}))).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&raw_cdp).unwrap()["error"]["code"],
        -32602
    );
    let oversized = vec![b'x'; crate::acp::limits::BROWSER_LIMITS.request_bytes + 1];
    assert_eq!(server.handle(&oversized), Err(BrowserMcpError::Capacity));
}

#[test]
fn wrong_session_expired_and_reused_token_are_rejected() {
    let (mut active, token) = server();
    assert!(!format!("{token:?}").contains(&token.0));
    let token = BrowserMcpToken::from_env_value(token.into_env_value()).unwrap();
    assert_eq!(
        active.connect(&token, "other-session", 1_001),
        Err(BrowserMcpError::Unauthorized)
    );
    let (_, foreign_token) = server();
    assert_eq!(
        active.connect(&foreign_token, "session-1", 1_001),
        Err(BrowserMcpError::Unauthorized)
    );
    active.connect(&token, "session-1", 1_001).unwrap();
    assert_eq!(
        active.connect(&token, "session-1", 1_002),
        Err(BrowserMcpError::Unauthorized)
    );
    let (mut expired, token) = server();
    assert_eq!(
        expired.connect(&token, "session-1", 61_001),
        Err(BrowserMcpError::Expired)
    );
}

#[test]
fn tools_call_routes_through_the_bounded_redacting_capability_dispatch() {
    struct MaliciousPort;
    impl BrowserCapability for MaliciousPort {
        fn perform(
            &mut self,
            _: &BrowserRequest,
        ) -> Result<BrowserResult, super::super::BrowserFailure> {
            Ok(BrowserResult::ReadNetwork {
                next_cursor: 1,
                entries: vec![NetworkEntry {
                    cursor: 1,
                    method: "GET".into(),
                    url: "https://user:pass@example.com/?token=do-not-leak".into(),
                    status: 200,
                }],
            })
        }
    }
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 7,
    };
    let (mut server, token) = BrowserMcpServer::issue(MaliciousPort, scope, 1_000);
    server.connect(&token, "session-1", 1_001).unwrap();
    let response = server.handle(&rpc("tools/call", json!({
        "name":"readNetwork","arguments":{"scope":{"sessionId":"session-1","tabId":7},"limit":1}
    }))).unwrap();
    let text = String::from_utf8(response).unwrap();
    assert!(!text.contains("do-not-leak"));
    assert!(!text.contains("user:pass"));
    assert!(text.contains("redacted"));
}

#[test]
fn scope_mismatch_never_reaches_the_port() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    let wrong = rpc(
        "tools/call",
        json!({"name":"snapshotPage","arguments":{
            "scope":{"sessionId":"session-2","tabId":7}
        }}),
    );
    let response = server.handle(&wrong).unwrap();
    let value: Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(value["id"], 1);
    assert_eq!(value["result"]["isError"], true);
    assert!(server.handle(&rpc("tools/list", json!({}))).is_ok());
}

#[test]
fn stdio_ndjson_is_bounded_and_emits_one_response_per_request() {
    let (mut server, token) = server();
    server.connect(&token, "session-1", 1_001).unwrap();
    let mut input = rpc("tools/list", json!({}));
    input.push(b'\n');
    input.extend_from_slice(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    input.push(b'\n');
    input.extend(rpc("tools/list", json!({})));
    input.push(b'\n');
    let mut output = Vec::new();
    server
        .serve_ndjson(&mut input.as_slice(), &mut output)
        .unwrap();
    assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 2);

    let oversized = vec![b'x'; BROWSER_LIMITS.request_bytes + 1];
    assert_eq!(
        server.serve_ndjson(&mut oversized.as_slice(), &mut Vec::new()),
        Err(BrowserMcpError::Capacity)
    );
}
