use serde_json::{Value, json};

use super::*;
use crate::acp::bindings::standard_acp::{
    PROFILES, StandardAcpProfile,
    translator::{AdapterEvent, ResourceMapper, StandardAcpTranslator},
};
use crate::acp::intent::{SafeResourceRef, ToolCategory, UnavailableResourceReason};

#[test]
fn curated_host_tools_advertise_browse_without_harness_names() {
    assert_eq!(CURATED_APPLY_FILE_TOOL_NAME, "sage.apply_file");
    assert_eq!(CURATED_BROWSE_TOOL_NAME, "_sage/browse");
    assert!(is_advertised_host_tool("_sage/browse"));
    assert!(!is_advertised_host_tool("sage.read_text"));
    assert!(is_advertised_host_tool("sage.apply_file"));
    assert!(!is_advertised_host_tool("sage.write_text"));
    assert!(!is_advertised_host_tool("shell"));
    assert!(!is_advertised_host_tool("codex.exec"));
    assert!(!is_advertised_host_tool("sage.browse"));
    assert!(!is_advertised_host_tool("sage_browse"));
    assert!(!is_advertised_host_tool("claude.browse"));
}

struct NoResources;

impl ResourceMapper for NoResources {
    fn map_absolute(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

/// The browse URL a standard ACP binding retains for a `tool_call` update.
fn retained_browse_input(profile: &'static StandardAcpProfile, update: Value) -> Option<String> {
    let mut translator = StandardAcpTranslator::new(profile, "session-1").unwrap();
    translator.begin_prompt("prompt-browse").unwrap();
    let frame = json!({
        "method": "session/update",
        "params": {"sessionId": "session-1", "update": update},
    });
    let events = translator
        .translate_notification(&serde_json::to_vec(&frame).unwrap(), &NoResources)
        .unwrap();
    let rendered = format!("{events:?}");
    assert!(
        !rendered.contains("example.com"),
        "{}: {rendered}",
        profile.harness_id
    );
    events.into_iter().find_map(|event| match event {
        AdapterEvent::Item {
            payload:
                NormalizedPayload::ToolCall {
                    category: ToolCategory::Fetch,
                    input_summary,
                    ..
                },
            ..
        } => Some(input_summary.map(|summary| summary.as_str().to_owned())),
        _ => None,
    })?
}

#[test]
fn every_standard_acp_binding_retains_the_same_admitted_browse_url() {
    let update = |title: &str, url: &str| {
        json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "browse-1",
            "kind": "fetch",
            "title": title,
            "status": "pending",
            "locations": [{"url": url}],
            "rawInput": {"url": "https://example.com/secret"}
        })
    };
    for profile in PROFILES {
        let harness = profile.harness_id;
        // The curated tool keeps only the admitted loopback location.
        assert_eq!(
            retained_browse_input(profile, update("_sage/browse", "http://127.0.0.1:5173/app")),
            Some("http://127.0.0.1:5173/app".into()),
            "{harness}"
        );
        // A harness-named title gains nothing over any other title.
        for title in ["claude.browse", "codex.browse", "gemini.browse", "fetch"] {
            assert_eq!(
                retained_browse_input(profile, update(title, "http://[::1]:8443/docs")),
                Some("http://[::1]:8443/docs".into()),
                "{harness} {title}"
            );
        }
        // A remote location is never retained, whichever binding saw it.
        assert_eq!(
            retained_browse_input(profile, update("_sage/browse", "https://example.com/docs")),
            None,
            "{harness}"
        );
    }
}

#[test]
fn initialize_capabilities_advertise_only_namespaced_curated_tools() {
    let capabilities = advertised_host_client_capabilities();
    assert_eq!(
        capabilities,
        json!({
            "fs":{"readTextFile":true,"writeTextFile":false},
            "terminal":false,
            "_meta":{"sage":{"tools":["sage.apply_file", "_sage/browse"]}}
        })
    );
    assert!(capabilities.get("availableTools").is_none());
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
