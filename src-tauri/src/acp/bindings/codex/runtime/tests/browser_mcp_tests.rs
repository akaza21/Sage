use super::*;

fn start_first_turn(runtime: &mut CodexRuntime) -> AdapterBatch {
    runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Continue without browser").unwrap(),
            client_message_id: RequiredText::new("client", "browser-failed-before-turn").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"method":"turn/started","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}
            }})),
            &Mapper,
        )
        .unwrap()
}

#[test]
fn new_and_resumed_codex_sessions_receive_browser_descriptor_without_token_value() {
    let mcp = crate::acp::mcp::McpAdmission::browser_stdio(
        r"C:\Sage\sage.exe".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            (
                "SAGE_BROWSER_MCP_SESSION".into(),
                "session-codex-browser".into(),
            ),
            ("SAGE_BROWSER_MCP_TOKEN".into(), "secret-token".into()),
        ],
    )
    .unwrap();
    let mut fresh = fixture_runtime();
    fresh.mcp = mcp.clone();
    initialize(&mut fresh);
    let started = fresh
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let start: Value = serde_json::from_slice(started.frame().payload()).unwrap();
    assert_eq!(start["method"], "thread/start");
    assert_eq!(start["params"]["approvalPolicy"], "never");
    assert_eq!(
        start["params"]["config"]["mcp_servers"]["sage-browser"]["default_tools_approval_mode"],
        "approve"
    );
    assert_eq!(
        start["params"]["config"]["mcp_servers"]["sage-browser"]["command"],
        r"C:\Sage\sage.exe"
    );
    assert_eq!(
        start["params"]["config"]["mcp_servers"]["sage-browser"]["env_vars"][3],
        "SAGE_BROWSER_MCP_TOKEN"
    );
    assert_eq!(
        start["params"]["config"]["mcp_servers"]["sage-browser"]["required"], false,
        "an unavailable browser helper must not prevent a Codex chat from starting"
    );
    assert_eq!(
        start["params"]["config"]["mcp_servers"]["sage-browser"]["startup_timeout_sec"],
        30
    );
    assert_eq!(
        start["params"]["config"]["shell_environment_policy"]["exclude"],
        json!([
            "SAGE_BROWSER_MCP_PIPE",
            "SAGE_BROWSER_MCP_HOST_PID",
            "SAGE_BROWSER_MCP_SESSION",
            "SAGE_BROWSER_MCP_TOKEN"
        ])
    );
    assert!(!start.to_string().contains("secret-token"));

    let mut resumed = fixture_runtime();
    resumed.mcp = mcp;
    initialize(&mut resumed);
    let request = resumed
        .command(AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("thread", "thread-codex-browser").unwrap(),
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let resume: Value = serde_json::from_slice(request.frame().payload()).unwrap();
    assert_eq!(resume["method"], "thread/resume");
    assert_eq!(resume["params"]["excludeTurns"], true);
    assert_eq!(
        resume["params"]["config"]["mcp_servers"]["sage-browser"],
        start["params"]["config"]["mcp_servers"]["sage-browser"]
    );
    assert_eq!(
        resume["params"]["config"]["shell_environment_policy"],
        start["params"]["config"]["shell_environment_policy"]
    );
    assert!(!resume.to_string().contains("secret-token"));
}

#[test]
fn failed_optional_browser_helper_is_reported_without_exposing_codex_error() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let notification = runtime
        .ingest(
            &frame(json!({
                "method": "mcpServer/startupStatus/updated",
                "params": {
                    "error": "MCP client for `sage-browser` failed to start: C:\\private\\sage.exe",
                    "failureReason": null,
                    "name": "sage-browser",
                    "status": "failed",
                    "threadId": "thread-1"
                }
            })),
            &Mapper,
        )
        .expect("optional browser helper failure must not stop the chat");

    assert!(matches!(notification.events(), [AdapterEvent::IgnoredLate]));
    let started = start_first_turn(&mut runtime);
    assert!(matches!(
        started.events(),
        [AdapterEvent::Turn { .. },
        AdapterEvent::Error(NormalizedPayload::Error { code, message, .. })]
            if code.as_str() == "browserMcpUnavailable"
                && message.as_str().contains("Browser tools are unavailable")
                && !message.as_str().contains("private")
    ));
}

#[test]
fn repeated_browser_helper_failure_is_reported_once_per_session() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let failed = || {
        frame(json!({
            "method": "mcpServer/startupStatus/updated",
            "params": {
                "error": "MCP client for `sage-browser` failed to start",
                "failureReason": null,
                "name": "sage-browser",
                "status": "failed",
                "threadId": "thread-1"
            }
        }))
    };

    let first = runtime.ingest(&failed(), &Mapper).unwrap();
    let second = runtime.ingest(&failed(), &Mapper).unwrap();
    assert!(matches!(first.events(), [AdapterEvent::IgnoredLate]));
    assert!(matches!(second.events(), [AdapterEvent::IgnoredLate]));
    let started = start_first_turn(&mut runtime);
    assert!(matches!(
        started.events(),
        [AdapterEvent::Turn { .. }, AdapterEvent::Error(_)]
    ));
    let later = runtime.ingest(&failed(), &Mapper).unwrap();
    assert!(matches!(later.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn browser_failure_before_first_prompt_is_reported_once_after_turn_bind() {
    let mut runtime = fixture_runtime();
    let token = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    runtime.mcp = crate::acp::mcp::McpAdmission::browser_stdio(
        r"C:\Sage\sage.exe".into(),
        [
            ("SAGE_BROWSER_MCP_PIPE".into(), "pipe".into()),
            ("SAGE_BROWSER_MCP_HOST_PID".into(), "123".into()),
            ("SAGE_BROWSER_MCP_SESSION".into(), "session".into()),
            ("SAGE_BROWSER_MCP_TOKEN".into(), token.into()),
        ],
    )
    .unwrap();
    ready(&mut runtime);
    let error = format!("MCP client for `sage-browser` failed to start: {token}");
    let failed = frame(json!({
        "method": "mcpServer/startupStatus/updated",
        "params": {
            "error": error,
            "failureReason": null,
            "name": "sage-browser",
            "status": "failed",
            "threadId": "thread-1"
        }
    }));
    let (_, diagnostics) = crate::test_diagnostics::capture_diagnostics(|| {
        for _ in 0..2 {
            let early = runtime.ingest(&failed, &Mapper).unwrap();
            assert!(matches!(early.events(), [AdapterEvent::IgnoredLate]));
        }
        let started = start_first_turn(&mut runtime);
        assert!(matches!(started.events(), [
                AdapterEvent::Turn { state: crate::acp::lifecycle::TurnState::Running, .. },
                AdapterEvent::Error(NormalizedPayload::Error { code, .. })
            ] if code.as_str() == "browserMcpUnavailable"));
    });
    assert_eq!(
        diagnostics
            .matches("Codex browser MCP startup failed")
            .count(),
        1
    );
    assert!(diagnostics.contains("MCP client for `sage-browser` failed to start: [redacted]"));
    assert!(diagnostics.contains(&format!("error_bytes={}", error.len())));
    assert!(!diagnostics.contains(token));
}

#[test]
fn browser_startup_warning_is_emitted_once_for_accepted_failure() {
    let mut runtime = fixture_runtime();
    runtime.mcp = crate::acp::mcp::McpAdmission::browser_stdio(
        r"C:\Sage\sage.exe".into(),
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
    ready(&mut runtime);
    let failure = |thread| {
        frame(json!({
            "method": "mcpServer/startupStatus/updated",
            "params": {
                "error": "MCP client for `sage-browser` failed to start",
                "failureReason": null,
                "name": "sage-browser",
                "status": "failed",
                "threadId": thread
            }
        }))
    };
    let (_, diagnostics) = crate::test_diagnostics::capture_diagnostics(|| {
        assert!(runtime.ingest(&failure("foreign-thread"), &Mapper).is_err());
        runtime.ingest(&failure("thread-1"), &Mapper).unwrap();
        runtime.ingest(&failure("thread-1"), &Mapper).unwrap();
        start_first_turn(&mut runtime);
    });
    assert_eq!(
        diagnostics
            .matches("Codex browser MCP startup failed")
            .count(),
        1
    );
}

#[test]
fn browser_helper_failure_for_a_foreign_thread_is_rejected() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let foreign = runtime.ingest(
        &frame(json!({
            "method": "mcpServer/startupStatus/updated",
            "params": {
                "error": "MCP client failed to start",
                "failureReason": null,
                "name": "sage-browser",
                "status": "failed",
                "threadId": "foreign-thread"
            }
        })),
        &Mapper,
    );
    assert!(foreign.is_err());
}
