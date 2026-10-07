use super::*;

#[test]
fn foreign_descriptor_cannot_claim_sage_browser_approval() {
    let foreign =
        McpServerDescriptor::stdio_for_test("sage-browser", "C:\\tools\\foreign.exe", &[], vec![]);
    assert!(matches!(
        McpAdmission::for_test(vec![foreign]),
        Err(McpDescriptorError::Invalid)
    ));

    let sage = McpAdmission::browser_stdio(
        "C:\\Sage\\sage.exe".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            ("SAGE_BROWSER_MCP_SESSION".into(), "session".into()),
            ("SAGE_BROWSER_MCP_TOKEN".into(), "token".into()),
        ],
    )
    .unwrap();
    assert_eq!(
        sage.codex_config().unwrap().unwrap()["mcp_servers"]["sage-browser"]["default_tools_approval_mode"],
        "approve"
    );
}

#[test]
fn browser_failure_detail_redacts_exact_token_before_bounding() {
    let admission = McpAdmission::browser_stdio(
        "C:\\Sage\\sage.exe".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            ("SAGE_BROWSER_MCP_SESSION".into(), "session".into()),
            (
                "SAGE_BROWSER_MCP_TOKEN".into(),
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            ),
        ],
    )
    .unwrap();
    let detail = admission.browser_failure_detail(&format!(
        "helper 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef exited: {} 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "x".repeat(1000)
    ));
    assert!(detail.contains("[redacted]"));
    assert!(!detail.contains("0123456789abcdef"));
    assert!(detail.len() <= 512);
}

#[test]
fn browser_failure_detail_withholds_truncated_token_fragment() {
    let admission = McpAdmission::browser_stdio(
        "C:\\Sage\\sage.exe".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            ("SAGE_BROWSER_MCP_SESSION".into(), "session".into()),
            (
                "SAGE_BROWSER_MCP_TOKEN".into(),
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
            ),
        ],
    )
    .unwrap();
    let detail = admission.browser_failure_detail("startup failed: 0123456789abcdef");
    assert_eq!(
        detail,
        "browser startup detail withheld; possible token fragment"
    );
    assert!(!detail.contains("0123456789abcdef"));
}

#[test]
fn browser_failure_detail_withholds_invalid_token_descriptor() {
    let admission = McpAdmission::browser_stdio(
        "C:\\Sage\\sage.exe".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            ("SAGE_BROWSER_MCP_SESSION".into(), "session".into()),
            ("SAGE_BROWSER_MCP_TOKEN".into(), "short-token".into()),
        ],
    )
    .unwrap();
    let detail = admission.browser_failure_detail("startup failed: short-token short");
    assert_eq!(
        detail,
        "browser startup detail withheld; invalid token descriptor"
    );
}

#[test]
fn stdio_wire_is_exact_and_env_debug_is_redacted() {
    let env = McpEnv::for_test("PUBLIC_MODE", "do-not-log");
    let descriptor = McpServerDescriptor::stdio_for_test(
        "files",
        "C:\\tools\\mcp.exe",
        &["--root", "C:\\repo"],
        vec![env.clone()],
    );
    assert!(!format!("{env:?} {descriptor:?}").contains("do-not-log"));
    let admission = McpAdmission::for_test(vec![descriptor]).unwrap();
    assert!(
        admission.codex_config().unwrap().unwrap()["mcp_servers"]["files"]
            .get("startup_timeout_sec")
            .is_none()
    );
    assert_eq!(
        admission.wire(McpCapabilities::default()),
        vec![json!({
            "name":"files","command":"C:\\tools\\mcp.exe",
            "args":["--root","C:\\repo"],
            "env":[{"name":"PUBLIC_MODE","value":"do-not-log"}]
        })]
    );
}

#[test]
fn transport_capabilities_filter_http_and_sse() {
    let admission = McpAdmission::for_test(vec![
        McpServerDescriptor::stdio_for_test("stdio", "/usr/bin/mcp", &[], vec![]),
        McpServerDescriptor::http_for_test("http", "https://example.com/mcp"),
        McpServerDescriptor::sse_for_test("sse", "https://example.com/events"),
    ])
    .unwrap();
    assert_eq!(admission.wire(McpCapabilities::default()).len(), 1);
    assert_eq!(
        admission
            .wire(McpCapabilities {
                http: true,
                sse: false
            })
            .len(),
        2
    );
    assert_eq!(
        admission
            .wire(McpCapabilities {
                http: true,
                sse: true
            })
            .len(),
        3
    );
}

#[test]
fn descriptor_bounds_and_invalid_credentials_are_rejected() {
    assert!(
        McpAdmission::for_test(vec![McpServerDescriptor::stdio_for_test(
            "x",
            "/usr/bin/mcp",
            &vec!["x"; MAX_ARGS],
            vec![]
        )])
        .is_ok()
    );
    assert!(matches!(
        McpAdmission::for_test(vec![McpServerDescriptor::stdio_for_test(
            "name",
            "relative-mcp",
            &[],
            vec![]
        )]),
        Err(McpDescriptorError::Invalid)
    ));
    assert!(matches!(
        McpAdmission::for_test(vec![McpServerDescriptor::http_for_test(
            "name",
            "https://user:pass@example.com/mcp"
        )]),
        Err(McpDescriptorError::Invalid)
    ));
    assert!(matches!(
        McpAdmission::for_test(vec![McpServerDescriptor::stdio_for_test(
            "x",
            "/usr/bin/mcp",
            &vec!["x"; MAX_ARGS + 1],
            vec![]
        )]),
        Err(McpDescriptorError::Capacity)
    ));
    let too_many = vec![
        McpServerDescriptor::stdio_for_test("x", "/usr/bin/mcp", &[], vec![]);
        MAX_SERVERS + 1
    ];
    assert!(McpAdmission::for_test(too_many[..MAX_SERVERS].to_vec()).is_ok());
    assert!(matches!(
        McpAdmission::for_test(too_many),
        Err(McpDescriptorError::Capacity)
    ));
}

#[test]
fn standard_acp_session_wire_keeps_empty_array_and_gates_remote_transports() {
    let profile = crate::acp::bindings::standard_acp::profile_for("opencode").unwrap();
    let initialize = json!({
        "jsonrpc":"2.0","id":1,"result":{
            "protocolVersion":1,
            "agentCapabilities":{"mcpCapabilities":{"http":true}}
        }
    });
    let mut empty = crate::acp::bindings::standard_acp::StandardAcpHandshake::new(profile);
    empty.initialize().unwrap();
    empty.accept_initialize(&initialize).unwrap();
    let wire = serde_json::to_string(&empty.new_session("C:/repo").unwrap()).unwrap();
    assert!(wire.contains("\"mcpServers\":[]"), "{wire}");

    let admission = McpAdmission::for_test(vec![
        McpServerDescriptor::stdio_for_test("files", "C:/tools/mcp.exe", &[], vec![]),
        McpServerDescriptor::http_for_test("http", "https://example.com/mcp"),
        McpServerDescriptor::sse_for_test("sse", "https://example.com/sse"),
    ])
    .unwrap();
    let mut injected =
        crate::acp::bindings::standard_acp::StandardAcpHandshake::with_mcp(profile, admission);
    injected.initialize().unwrap();
    injected.accept_initialize(&initialize).unwrap();
    let wire = injected.new_session("C:/repo").unwrap();
    assert_eq!(wire["params"]["mcpServers"].as_array().unwrap().len(), 2);
    assert_eq!(wire["params"]["mcpServers"][0]["name"], "files");
    assert_eq!(wire["params"]["mcpServers"][1]["type"], "http");
}
