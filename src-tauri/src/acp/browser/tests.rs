use super::bounds::*;
use super::*;
use crate::acp::limits::BROWSER_LIMITS;

fn scope(tab_id: u64) -> BrowserScope {
    BrowserScope {
        session_id: "session-1".into(),
        tab_id,
    }
}

#[test]
fn open_tab_contract_admits_only_public_urls_without_an_existing_tab() {
    for url in ["https://example.com/path", "http://example.com/"] {
        let request = BrowserRequest {
            scope: scope(0),
            command: BrowserCommand::OpenTab { url: url.into() },
        };
        assert_eq!(validate_request(&request), Ok(()));
    }
    for url in [
        "file:///secret",
        "data:text/html,hi",
        "javascript:alert(1)",
        "https://user:password@example.com/",
    ] {
        let request = BrowserRequest {
            scope: scope(0),
            command: BrowserCommand::OpenTab { url: url.into() },
        };
        assert_eq!(
            validate_request(&request),
            Err(BrowserFailure::InvalidRequest)
        );
    }
    let request = BrowserRequest {
        scope: scope(1),
        command: BrowserCommand::OpenTab {
            url: "https://example.com/".into(),
        },
    };
    assert_eq!(
        validate_request(&request),
        Err(BrowserFailure::InvalidRequest)
    );
}

#[test]
fn unknown_json_fields_and_oversized_frames_fail_before_dispatch() {
    let mut fake = FakeBrowserCapability::default();
    let unknown = br#"{"scope":{"sessionId":"session-1","tabId":1},"command":{"kind":"snapshotPage"},"rawCdp":"Runtime.evaluate"}"#;
    assert_eq!(
        dispatch_json(&mut fake, unknown),
        Err(BrowserFailure::InvalidRequest)
    );
    assert_eq!(fake.action_count(), 0);
    let nested_unknown = br#"{"scope":{"sessionId":"session-1","tabId":1,"origin":"https://example.com"},"command":{"kind":"snapshotPage"}}"#;
    assert_eq!(
        dispatch_json(&mut fake, nested_unknown),
        Err(BrowserFailure::InvalidRequest)
    );
    let command_unknown = br#"{"scope":{"sessionId":"session-1","tabId":1},"command":{"kind":"snapshotPage","javascript":"1+1"}}"#;
    assert_eq!(
        dispatch_json(&mut fake, command_unknown),
        Err(BrowserFailure::InvalidRequest)
    );
    let oversized = vec![b' '; BROWSER_LIMITS.request_bytes + 1];
    assert_eq!(
        dispatch_json(&mut fake, &oversized),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(fake.action_count(), 0);
}

#[test]
fn stale_and_cross_tab_handles_fail_without_actions() {
    let mut fake = FakeBrowserCapability::default();
    fake.open_tab(scope(1));
    fake.open_tab(scope(2));
    let handle = fake.issue_handle(&scope(1)).unwrap();
    let cross_tab = BrowserRequest::act(
        scope(2),
        BrowserAction::Click {
            handle: handle.clone(),
        },
    );
    assert_eq!(
        dispatch(&mut fake, cross_tab),
        Err(BrowserFailure::StaleHandle)
    );
    assert_eq!(fake.action_count(), 0);
    fake.advance_navigation(&scope(1)).unwrap();
    let stale = BrowserRequest::act(scope(1), BrowserAction::Click { handle });
    assert_eq!(dispatch(&mut fake, stale), Err(BrowserFailure::StaleHandle));
    assert_eq!(fake.action_count(), 0);
}

#[test]
fn fill_values_are_redacted_in_debug_and_json() {
    let request = BrowserRequest::act(
        scope(1),
        BrowserAction::Fill {
            handle: ElementHandle::from_token("opaque-handle".into()),
            value: FillSecret::new("do-not-echo".into()),
        },
    );
    assert!(!format!("{request:?}").contains("do-not-echo"));
    assert!(
        !serde_json::to_string(&request)
            .unwrap()
            .contains("do-not-echo")
    );
}

#[test]
fn each_text_boundary_accepts_its_cap_and_rejects_one_more_byte() {
    let limits = BROWSER_LIMITS;
    let cases = [
        ("sessionId", limits.session_id_bytes),
        ("url", limits.url_bytes),
        ("handle", limits.handle_bytes),
        ("key", limits.key_bytes),
        ("fill", limits.fill_bytes),
        ("waitText", limits.wait_text_bytes),
    ];
    for (field, cap) in cases {
        assert!(
            admit_text(field, &"x".repeat(cap)).is_ok(),
            "{field} at cap"
        );
        assert_eq!(
            admit_text(field, &"x".repeat(cap + 1)),
            Err(BrowserFailure::Capacity),
            "{field} over cap"
        );
    }
}

#[test]
fn result_lists_bytes_and_dimensions_have_closed_caps() {
    let limits = BROWSER_LIMITS;
    assert!(admit_count(limits.snapshot_nodes, limits.snapshot_nodes).is_ok());
    assert_eq!(
        admit_count(limits.snapshot_nodes + 1, limits.snapshot_nodes),
        Err(BrowserFailure::Capacity)
    );
    assert!(admit_count(limits.diagnostic_entries, limits.diagnostic_entries).is_ok());
    assert_eq!(
        admit_count(limits.diagnostic_entries + 1, limits.diagnostic_entries),
        Err(BrowserFailure::Capacity)
    );
    assert!(
        admit_screenshot(
            limits.screenshot_width,
            limits.screenshot_height,
            limits.screenshot_bytes
        )
        .is_ok()
    );
    assert_eq!(
        admit_screenshot(limits.screenshot_width + 1, 1, 1),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(
        admit_screenshot(1, limits.screenshot_height + 1, 1),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(
        admit_screenshot(1, 1, limits.screenshot_bytes + 1),
        Err(BrowserFailure::Capacity)
    );
}

#[test]
fn request_numeric_caps_reject_one_over_before_the_port_runs() {
    let mut fake = FakeBrowserCapability::default();
    fake.open_tab(scope(1));
    let request = |command| BrowserRequest {
        scope: scope(1),
        command,
    };
    assert!(
        validate_request(&request(BrowserCommand::WaitFor {
            condition: WaitCondition::Text {
                text: "ready".into()
            },
            timeout_ms: BROWSER_LIMITS.wait_timeout_ms,
        }))
        .is_ok()
    );
    assert_eq!(
        dispatch(
            &mut fake,
            request(BrowserCommand::WaitFor {
                condition: WaitCondition::Text {
                    text: "ready".into()
                },
                timeout_ms: BROWSER_LIMITS.wait_timeout_ms + 1,
            })
        ),
        Err(BrowserFailure::Capacity)
    );
    assert!(
        validate_request(&request(BrowserCommand::ReadConsole {
            cursor: None,
            limit: BROWSER_LIMITS.diagnostic_entries
        }))
        .is_ok()
    );
    assert_eq!(
        dispatch(
            &mut fake,
            request(BrowserCommand::ReadNetwork {
                cursor: None,
                limit: BROWSER_LIMITS.diagnostic_entries + 1
            })
        ),
        Err(BrowserFailure::Capacity)
    );
    let handle = fake.issue_handle(&scope(1)).unwrap();
    assert!(
        validate_request(&BrowserRequest::act(
            scope(1),
            BrowserAction::Scroll {
                handle: handle.clone(),
                delta_x: BROWSER_LIMITS.scroll_delta,
                delta_y: 0,
            }
        ))
        .is_ok()
    );
    assert_eq!(
        dispatch(
            &mut fake,
            BrowserRequest::act(
                scope(1),
                BrowserAction::Scroll {
                    handle,
                    delta_x: BROWSER_LIMITS.scroll_delta + 1,
                    delta_y: 0,
                }
            )
        ),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(fake.action_count(), 0);
}

#[test]
fn result_projection_caps_apply_to_every_returned_collection() {
    let node = SnapshotNode {
        handle: ElementHandle::from_token("node".into()),
        role: "button".into(),
        name: "Open".into(),
    };
    let snapshot = |count| BrowserResult::SnapshotPage {
        navigation_epoch: 1,
        text: String::new(),
        nodes: vec![node.clone(); count],
    };
    assert!(validate_result(&snapshot(BROWSER_LIMITS.snapshot_nodes)).is_ok());
    assert_eq!(
        validate_result(&snapshot(BROWSER_LIMITS.snapshot_nodes + 1)),
        Err(BrowserFailure::Capacity)
    );
    let entry = ConsoleEntry {
        cursor: 1,
        level: "info".into(),
        text: "ready".into(),
    };
    let console = |count| BrowserResult::ReadConsole {
        next_cursor: 1,
        entries: vec![entry.clone(); count],
    };
    assert!(validate_result(&console(BROWSER_LIMITS.diagnostic_entries)).is_ok());
    assert_eq!(
        validate_result(&console(BROWSER_LIMITS.diagnostic_entries + 1)),
        Err(BrowserFailure::Capacity)
    );
    let entry = NetworkEntry {
        cursor: 1,
        method: "GET".into(),
        url: "https://example.com".into(),
        status: 200,
    };
    let network = |count| BrowserResult::ReadNetwork {
        next_cursor: 1,
        entries: vec![entry.clone(); count],
    };
    assert!(validate_result(&network(BROWSER_LIMITS.diagnostic_entries)).is_ok());
    assert_eq!(
        validate_result(&network(BROWSER_LIMITS.diagnostic_entries + 1)),
        Err(BrowserFailure::Capacity)
    );
}

#[test]
fn navigation_rejects_non_web_credentials_and_control_characters() {
    let request = |url: &str| BrowserRequest {
        scope: scope(1),
        command: BrowserCommand::Navigate { url: url.into() },
    };
    assert!(validate_request(&request("https://example.com/path")).is_ok());
    for url in [
        "file:///secret",
        "javascript:1+1",
        "https://user:pass@example.com",
        "https://example.com/\nnext",
    ] {
        assert_eq!(
            validate_request(&request(url)),
            Err(BrowserFailure::InvalidRequest)
        );
    }
}

#[test]
fn dispatched_result_carries_the_exact_session_and_tab_scope() {
    let mut fake = FakeBrowserCapability::default();
    fake.open_tab(scope(7));
    let response = dispatch(
        &mut fake,
        BrowserRequest {
            scope: scope(7),
            command: BrowserCommand::SnapshotPage {},
        },
    )
    .unwrap();
    assert_eq!(response.scope, scope(7));
    assert!(matches!(
        response.result,
        BrowserResult::SnapshotPage { .. }
    ));
}

mod conformance;
mod redaction;
