use super::*;
use serde_json::json;

fn inventory(result: Value) -> Option<McpInventory> {
    McpInventory::from_codex(result.as_object().unwrap())
}

#[test]
fn a_codex_list_becomes_named_servers_with_states_counts_and_origins() {
    let listed = inventory(json!({
        "data": [
            {
                "name": "notion",
                "authStatus": "notLoggedIn",
                "runtimeStatus": "authenticationRequired",
                "httpOrigin": "https://mcp.notion.com",
                "serverInfo": {"name": "notion", "title": "Notion", "version": "1.2.0"},
                "tools": {},
                "resources": [],
                "resourceTemplates": []
            },
            {
                "name": "files",
                "authStatus": "unsupported",
                "runtimeStatus": "connected",
                "httpOrigin": null,
                "tools": {"read": {"name": "read", "inputSchema": {}}, "write": {"name": "write", "inputSchema": {}}},
                "resources": [{"name": "a", "uri": "file:///a"}],
                "resourceTemplates": []
            }
        ],
        "nextCursor": null
    }))
    .unwrap();
    assert!(!listed.truncated);
    // Sign-in is offered only where Sage can open the browser.
    assert_eq!(
        listed.actions,
        McpActions {
            reload: true,
            sign_in: cfg!(windows),
            reconnect: false,
        }
    );
    assert_eq!(
        listed.servers,
        vec![
            McpServer {
                name: "files".into(),
                title: None,
                version: None,
                connection: McpConnection::Connected,
                auth: McpAuth::None,
                tools: 2,
                resources: 1,
                error: None,
                origin: None,
            },
            McpServer {
                name: "notion".into(),
                title: Some("Notion".into()),
                version: Some("1.2.0".into()),
                connection: McpConnection::SignInRequired,
                auth: McpAuth::SignedOut,
                tools: 0,
                resources: 0,
                error: None,
                origin: Some("https://mcp.notion.com".into()),
            },
        ]
    );
}

#[test]
fn unknown_states_unnamed_servers_and_a_missing_list_are_handled() {
    assert_eq!(inventory(json!({"nextCursor": null})), None);
    let listed = inventory(json!({"data": [
        {"name": "", "authStatus": "oAuth", "tools": {}, "resources": [], "resourceTemplates": []},
        {"name": "a\tb", "authStatus": "oAuth", "tools": {}, "resources": [], "resourceTemplates": []},
        {"name": " padded", "authStatus": "oAuth", "tools": {}, "resources": [], "resourceTemplates": []},
        {"authStatus": "oAuth"},
        {"name": "later", "authStatus": "somethingNew", "runtimeStatus": "rebooting",
         "tools": {}, "resources": [], "resourceTemplates": []}
    ]}))
    .unwrap();
    assert_eq!(listed.servers.len(), 1);
    assert_eq!(listed.servers[0].connection, McpConnection::Unknown);
    assert_eq!(listed.servers[0].auth, McpAuth::Unknown);
}

#[test]
fn text_is_one_clipped_line_and_origins_drop_paths_and_credentials() {
    let error = format!("bad\nthing\u{7}{}", "é".repeat(400));
    let listed = inventory(json!({"data": [
        {"name": "a", "authStatus": "bearerToken", "toolsError": error,
         "httpOrigin": "https://user:secret@example.com:8443/path?q=1",
         "tools": {}, "resources": [], "resourceTemplates": []},
        {"name": "c", "authStatus": "bearerToken",
         "httpOrigin": "https://example.com:8443/path?q=1",
         "tools": {}, "resources": [], "resourceTemplates": []},
        {"name": "d", "authStatus": "bearerToken", "httpOrigin": "file:///etc/passwd",
         "tools": {}, "resources": [], "resourceTemplates": []}
    ]}))
    .unwrap();
    let [a, c, d] = listed.servers.as_slice() else {
        panic!("three servers")
    };
    assert_eq!(a.name, "a");
    assert_eq!(a.auth, McpAuth::Token);
    let error = a.error.as_deref().unwrap();
    assert!(error.starts_with("bad thing é"));
    assert!(error.len() <= 512 && !error.chars().any(char::is_control));
    assert_eq!(a.origin, None, "an origin with credentials is not shown");
    assert_eq!(c.origin.as_deref(), Some("https://example.com:8443"));
    assert_eq!(d.origin, None);
}

#[test]
fn a_long_or_paged_list_is_marked_truncated() {
    let server = |index: usize| {
        json!({"name": format!("s{index:03}"), "authStatus": "unsupported",
               "tools": {}, "resources": [], "resourceTemplates": []})
    };
    let many: Vec<_> = (0..MAX_LISTED_SERVERS + 3).map(server).collect();
    let listed = inventory(json!({"data": many})).unwrap();
    assert!(listed.truncated);
    assert_eq!(listed.servers.len(), MAX_LISTED_SERVERS);
    let paged = inventory(json!({"data": [server(1)], "nextCursor": "page-2"})).unwrap();
    assert!(paged.truncated);
}

#[test]
fn a_claude_status_reply_lists_servers_without_their_configuration() {
    let listed = McpInventory::from_claude(&json!({"mcpServers": [
        {
            "name": "linear",
            "status": "needs-auth",
            "scope": "user",
            "config": {
                "type": "http",
                "url": "https://mcp.linear.app/mcp/secret-path?token=abc",
                "headers": {"Authorization": "Bearer hidden"}
            }
        },
        {
            "name": "files",
            "status": "connected",
            "serverInfo": {"name": "File Server", "version": "0.4.1"},
            "config": {"type": "stdio", "command": "/opt/files", "args": ["--key", "hidden"]},
            "tools": [{"name": "read"}, {"name": "write"}, {"name": "list"}]
        },
        {"name": "flaky", "status": "failed", "error": "spawn ENOENT\n  at x"},
        {"name": "warming", "status": "pending"},
        {"name": "off", "status": "disabled"},
        {"name": "next", "status": "somethingNew"},
        {"name": " padded", "status": "connected"},
        {"status": "connected"}
    ]}))
    .unwrap();
    assert_eq!(
        listed.actions,
        McpActions {
            reload: false,
            sign_in: false,
            reconnect: true,
        }
    );
    assert!(!listed.truncated);
    let states: Vec<_> = listed
        .servers
        .iter()
        .map(|server| (server.name.as_str(), server.connection))
        .collect();
    assert_eq!(
        states,
        vec![
            ("files", McpConnection::Connected),
            ("flaky", McpConnection::Failed),
            ("linear", McpConnection::SignInRequired),
            ("next", McpConnection::Unknown),
            ("off", McpConnection::Disabled),
            ("warming", McpConnection::Starting),
        ]
    );
    let files = &listed.servers[0];
    assert_eq!(files.title.as_deref(), Some("File Server"));
    assert_eq!(files.version.as_deref(), Some("0.4.1"));
    assert_eq!((files.tools, files.origin.as_deref()), (3, None));
    assert_eq!(
        listed.servers[1].error.as_deref(),
        Some("spawn ENOENT at x")
    );
    let linear = &listed.servers[2];
    assert_eq!(linear.auth, McpAuth::SignedOut);
    assert_eq!(linear.origin.as_deref(), Some("https://mcp.linear.app"));
    let shown = serde_json::to_string(&listed).unwrap();
    for secret in ["hidden", "secret-path", "token=abc", "/opt/files", "user"] {
        assert!(!shown.contains(secret), "{secret} reached the WebView");
    }
    assert_eq!(McpInventory::from_claude(&json!({})), None);
}

#[test]
fn only_a_plain_https_sign_in_url_is_admitted() {
    let admitted = McpAuthUrl::parse("https://auth.example.com/authorize?client_id=x&state=y");
    assert!(admitted.is_some());
    assert_eq!(format!("{:?}", admitted.unwrap()), "McpAuthUrl([redacted])");
    for refused in [
        "",
        "http://auth.example.com/authorize",
        "javascript:alert(1)",
        "file:///c:/windows/system32/calc.exe",
        "https://user:pass@auth.example.com/",
        "https://auth.example.com/\r\nx",
        "ms-settings:privacy",
    ] {
        assert!(McpAuthUrl::parse(refused).is_none(), "{refused}");
    }
    let long = format!("https://auth.example.com/?q={}", "a".repeat(4096));
    assert!(McpAuthUrl::parse(&long).is_none());
}
