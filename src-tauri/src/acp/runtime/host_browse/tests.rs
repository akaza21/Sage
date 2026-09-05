use serde_json::json;

use super::*;

#[test]
fn curated_host_tools_advertise_browse_without_harness_names() {
    assert_eq!(CURATED_READ_TOOL_NAME, "sage.read_text");
    assert_eq!(CURATED_APPLY_FILE_TOOL_NAME, "sage.apply_file");
    assert_eq!(CURATED_BROWSE_TOOL_NAME, "_sage/browse");
    assert!(is_advertised_host_tool("_sage/browse"));
    assert!(is_advertised_host_tool("sage.read_text"));
    assert!(is_advertised_host_tool("sage.apply_file"));
    assert!(!is_advertised_host_tool("sage.write_text"));
    assert!(!is_advertised_host_tool("shell"));
    assert!(!is_advertised_host_tool("codex.exec"));
    assert!(!is_advertised_host_tool("sage.browse"));
    assert!(!is_advertised_host_tool("sage_browse"));
    assert!(!is_advertised_host_tool("claude.browse"));
    let source = include_str!("../host_browse.rs");
    assert!(!source.to_ascii_lowercase().contains("codex"));
    assert!(!source.to_ascii_lowercase().contains("claude"));
    assert!(!source.to_ascii_lowercase().contains("oh-my-pi"));
}

#[test]
fn initialize_capabilities_advertise_only_namespaced_curated_tools() {
    let capabilities = advertised_host_client_capabilities();
    assert_eq!(
        capabilities,
        json!({"_meta":{"sage":{"tools":[
            "sage.read_text", "sage.apply_file", "_sage/browse"
        ]}}})
    );
    assert!(capabilities.get("availableTools").is_none());
    assert!(capabilities.get("fs").is_none());
    assert!(capabilities.get("terminal").is_none());
    assert!(!is_advertised_host_tool("sage.write_text"));
    assert!(!is_advertised_host_tool("fs/read_text_file"));
}

#[test]
fn dispatch_opens_an_admitted_loopback_url() {
    let mut host = BrowseHost::default();
    let opened = host
        .dispatch(&BrowseIntent {
            chat_id: "chat:a".into(),
            session_id: Some("session-1".into()),
            url: "http://127.0.0.1:5173/app".into(),
        })
        .expect("admitted");
    assert_eq!(opened.href, "http://127.0.0.1:5173/app");
    assert_eq!(
        host.session_for_chat("chat:a")
            .map(|session| session.href.as_str()),
        Some("http://127.0.0.1:5173/app")
    );
}

#[test]
fn rejected_urls_stay_closed() {
    let mut host = BrowseHost::default();
    let opened = host.dispatch(&BrowseIntent {
        chat_id: "chat:a".into(),
        session_id: Some("session-1".into()),
        url: "http://127.0.0.1:5173/app".into(),
    });
    assert!(opened.is_some());
    for url in [
        "",
        "http://127.0.0.1/app",
        "http://localhost:5173/",
        "https://example.com/docs",
        "http://user:pass@127.0.0.1:5173/",
        "file:///tmp/index.html",
        "javascript:alert(1)",
        "data:text/html,hi",
    ] {
        assert!(
            host.dispatch(&BrowseIntent {
                chat_id: "chat:a".into(),
                session_id: Some("session-1".into()),
                url: url.into(),
            })
            .is_none(),
            "{url}"
        );
    }
    assert_eq!(
        host.session_for_chat("chat:a")
            .map(|session| session.href.as_str()),
        Some("http://127.0.0.1:5173/app")
    );
}

#[test]
fn browse_is_scoped_to_chat_and_session() {
    let mut host = BrowseHost::default();
    assert!(
        host.dispatch(&BrowseIntent {
            chat_id: "chat:a".into(),
            session_id: Some("session-1".into()),
            url: "http://127.0.0.1:4173/".into(),
        })
        .is_some()
    );
    assert!(
        host.dispatch(&BrowseIntent {
            chat_id: "chat:a".into(),
            session_id: Some("session-2".into()),
            url: "http://127.0.0.1:3000/".into(),
        })
        .is_none()
    );
    assert_eq!(
        host.session_for_chat("chat:a")
            .map(|session| session.href.as_str()),
        Some("http://127.0.0.1:4173/")
    );
    assert!(host.session_for_chat("chat:b").is_none());
    assert!(
        host.dispatch(&BrowseIntent {
            chat_id: "chat:b".into(),
            session_id: Some("session-1".into()),
            url: "https://[::1]:8443/".into(),
        })
        .is_some()
    );
    assert_eq!(
        host.session_for_chat("chat:b")
            .map(|session| session.href.as_str()),
        Some("https://[::1]:8443/")
    );
    assert_eq!(
        host.session_for_chat("chat:a")
            .map(|session| session.href.as_str()),
        Some("http://127.0.0.1:4173/")
    );
}

#[test]
fn browse_cap_replaces_the_oldest_chat() {
    let mut host = BrowseHost::default();
    assert_eq!(MAX_HARNESS_BROWSE_SESSIONS, 2);
    assert!(
        host.dispatch(&BrowseIntent {
            chat_id: "chat:a".into(),
            session_id: None,
            url: "http://127.0.0.1:5173/".into(),
        })
        .is_some()
    );
    assert!(
        host.dispatch(&BrowseIntent {
            chat_id: "chat:b".into(),
            session_id: None,
            url: "http://127.0.0.1:4173/".into(),
        })
        .is_some()
    );
    assert!(
        host.dispatch(&BrowseIntent {
            chat_id: "chat:c".into(),
            session_id: None,
            url: "http://127.0.0.1:3000/".into(),
        })
        .is_some()
    );
    assert!(host.session_for_chat("chat:a").is_none());
    assert_eq!(
        host.session_for_chat("chat:c")
            .map(|session| session.href.as_str()),
        Some("http://127.0.0.1:3000/")
    );
}

#[test]
fn host_browse_request_is_session_scoped_and_fail_closed() {
    let admitted = parse_host_browse_request(
        "_sage/browse",
        json!({"sessionId":"session-1","url":"http://127.0.0.1:5173/"}).as_object(),
    )
    .expect("request");
    assert!(host_browse_session_matches(&admitted, "session-1"));
    assert!(!host_browse_session_matches(&admitted, "session-2"));
    assert!(parse_host_browse_request("sage.browse", json!({}).as_object()).is_none());
    assert!(parse_host_browse_request("sage_browse", json!({}).as_object()).is_none());
    assert!(parse_host_browse_request("fs/read_text_file", json!({}).as_object()).is_none());
    assert_eq!(
        admit_browse_url("http://127.0.0.1:5173/app").as_deref(),
        Some("http://127.0.0.1:5173/app")
    );
    assert_eq!(
        admit_browse_url("https://[::1]:8443/").as_deref(),
        Some("https://[::1]:8443/")
    );
    let update = json!({
        "kind":"fetch",
        "title":"_sage/browse",
        "locations":[{"url":"http://127.0.0.1:4173/preview"}],
        "rawInput":{"url":"https://example.com/secret"}
    });
    assert_eq!(
        admitted_browse_href_from_tool(update.as_object().unwrap()).as_deref(),
        Some("http://127.0.0.1:4173/preview")
    );
}
