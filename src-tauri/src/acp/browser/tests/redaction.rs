use super::*;
#[test]
fn malicious_port_network_and_console_secrets_are_redacted_before_return() {
    struct MaliciousPort;
    impl BrowserCapability for MaliciousPort {
        fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            match request.command {
                BrowserCommand::ReadNetwork { .. } => Ok(BrowserResult::ReadNetwork {
                    next_cursor: 1,
                    entries: vec![NetworkEntry {
                        cursor: 1,
                        method: "GET".into(),
                        url: "https://user:pass@example.com/?access_token=abc&safe=ok#sig=xyz".into(),
                        status: 200,
                    }],
                }),
                BrowserCommand::Navigate { .. } => Ok(BrowserResult::Navigate {
                    url: "https://user:pass@example.com/?token=redirect-secret&safe=ok".into(),
                    navigation_epoch: 2,
                }),
                _ => Ok(BrowserResult::ReadConsole {
                    next_cursor: 1,
                    entries: vec![ConsoleEntry {
                        cursor: 1,
                        level: "error".into(),
                        text: "failed https://user:pass@example.com/?token=abc#session=xyz auth=private safe=ok".into(),
                    }],
                }),
            }
        }
    }
    let mut port = MaliciousPort;
    for command in [
        BrowserCommand::ReadNetwork {
            cursor: None,
            limit: 1,
        },
        BrowserCommand::ReadConsole {
            cursor: None,
            limit: 1,
        },
        BrowserCommand::Navigate {
            url: "https://example.com".into(),
        },
    ] {
        let response = dispatch(
            &mut port,
            BrowserRequest {
                scope: scope(1),
                command,
            },
        )
        .unwrap();
        let json = serde_json::to_string(&response).unwrap();
        for secret in ["user", "pass", "abc", "xyz", "private", "redirect-secret"] {
            assert!(!json.contains(secret), "leaked {secret}: {json}");
        }
        assert!(json.contains("safe=ok"));
    }
}

#[test]
fn malicious_port_snapshot_secrets_are_redacted_before_return() {
    struct Port;
    impl BrowserCapability for Port {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            Ok(BrowserResult::SnapshotPage {
                navigation_epoch: 1,
                text: "Page https://user:pass@example.com/?token=page-secret Authorization: Bearer header-secret".into(),
                nodes: vec![SnapshotNode {
                    handle: ElementHandle::from_token("opaque-handle".into()),
                    role: "textbox".into(),
                    name: "api-key: name-secret".into(),
                }],
            })
        }
    }
    let response = dispatch(
        &mut Port,
        BrowserRequest {
            scope: scope(1),
            command: BrowserCommand::SnapshotPage {},
        },
    )
    .unwrap();
    let json = serde_json::to_string(&response).unwrap();
    for secret in [
        "user",
        "pass",
        "page-secret",
        "header-secret",
        "name-secret",
    ] {
        assert!(!json.contains(secret), "{json}");
    }
    assert!(json.contains("opaque-handle"));
}

#[test]
fn semicolon_and_encoded_query_keys_do_not_escape_redaction() {
    struct Port;
    impl BrowserCapability for Port {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            Ok(BrowserResult::ReadNetwork {
                next_cursor: 1,
                entries: vec![NetworkEntry {
                    cursor: 1,
                    method: "GET".into(),
                    url: "https://example.com/?safe=ok;token=semicolon-secret&%61pi_key=encoded-secret".into(),
                    status: 200,
                }],
            })
        }
    }
    let response = dispatch(
        &mut Port,
        BrowserRequest {
            scope: scope(1),
            command: BrowserCommand::ReadNetwork {
                cursor: None,
                limit: 1,
            },
        },
    )
    .unwrap();
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("safe=ok"));
    assert!(!json.contains("semicolon-secret"), "{json}");
    assert!(!json.contains("encoded-secret"), "{json}");
}

#[test]
fn console_non_web_urls_are_redacted() {
    struct Port;
    impl BrowserCapability for Port {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            Ok(BrowserResult::ReadConsole {
                next_cursor: 1,
                entries: vec![ConsoleEntry {
                    cursor: 1,
                    level: "error".into(),
                    text: "data:text/plain,data-secret javascript:alert('script-secret') file:///private/file-secret".into(),
                }],
            })
        }
    }
    let response = dispatch(
        &mut Port,
        BrowserRequest {
            scope: scope(1),
            command: BrowserCommand::ReadConsole {
                cursor: None,
                limit: 1,
            },
        },
    )
    .unwrap();
    let json = serde_json::to_string(&response).unwrap();
    for secret in ["data-secret", "script-secret", "file-secret"] {
        assert!(!json.contains(secret), "{json}");
    }
}

#[test]
fn redacted_url_is_bounded_after_redaction() {
    struct Port;
    impl BrowserCapability for Port {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            Ok(BrowserResult::ReadNetwork {
                next_cursor: 1,
                entries: vec![NetworkEntry {
                    cursor: 1,
                    method: "GET".into(),
                    url: format!(
                        "https://example.com/?token={}",
                        "x".repeat(BROWSER_LIMITS.url_bytes)
                    ),
                    status: 200,
                }],
            })
        }
    }
    let response = dispatch(
        &mut Port,
        BrowserRequest {
            scope: scope(1),
            command: BrowserCommand::ReadNetwork {
                cursor: None,
                limit: 1,
            },
        },
    )
    .unwrap();
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("redacted"));
    assert!(!json.contains(&"x".repeat(64)));

    struct OversizedSafePort;
    impl BrowserCapability for OversizedSafePort {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            Ok(BrowserResult::ReadNetwork {
                next_cursor: 1,
                entries: vec![NetworkEntry {
                    cursor: 1,
                    method: "GET".into(),
                    // Not secret-shaped: a run of one letter would read as
                    // base64 and be redacted.
                    url: format!(
                        "https://example.com/?safe={}",
                        "x-".repeat(BROWSER_LIMITS.url_bytes)
                    ),
                    status: 200,
                }],
            })
        }
    }
    assert_eq!(
        dispatch(
            &mut OversizedSafePort,
            BrowserRequest {
                scope: scope(1),
                command: BrowserCommand::ReadNetwork {
                    cursor: None,
                    limit: 1
                },
            }
        ),
        Err(BrowserFailure::Capacity)
    );
}

#[test]
fn console_authorization_headers_hide_all_schemes_and_values() {
    struct Port;
    impl BrowserCapability for Port {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            Ok(BrowserResult::ReadConsole {
                next_cursor: 1,
                entries: vec![ConsoleEntry {
                    cursor: 1,
                    level: "error".into(),
                    text: "Authorization: Basic basic-secret\nPROXY-AUTHORIZATION : Digest digest-secret\nx-api-key: custom-secret\nAuthorization: Custom custom-secret".into(),
                }],
            })
        }
    }
    let response = dispatch(
        &mut Port,
        BrowserRequest {
            scope: scope(1),
            command: BrowserCommand::ReadConsole {
                cursor: None,
                limit: 1,
            },
        },
    )
    .unwrap();
    let json = serde_json::to_string(&response).unwrap();
    for secret in ["basic-secret", "digest-secret", "custom-secret"] {
        assert!(!json.contains(secret), "{json}");
    }
}

#[test]
fn console_bare_tokens_are_redacted_without_erasing_plain_text() {
    struct Port;
    impl BrowserCapability for Port {
        fn perform(&mut self, _: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
            Ok(BrowserResult::ReadConsole {
                next_cursor: 1,
                entries: vec![ConsoleEntry {
                    cursor: 1,
                    level: "error".into(),
                    text: "failed eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.signature123 ghp_abcdefghijklmnopqrstuvwx github_pat_abcdefghijklmnopqrstuvwx sk-ant-abcdefghijklmnopqrstuvwx xoxb-abcdefghijklmnopqrstuvwx AKIAABCDEFGHIJKLMNOP request denied".into(),
                }],
            })
        }
    }
    let response = dispatch(
        &mut Port,
        BrowserRequest {
            scope: scope(1),
            command: BrowserCommand::ReadConsole {
                cursor: None,
                limit: 1,
            },
        },
    )
    .unwrap();
    let json = serde_json::to_string(&response).unwrap();
    for secret in [
        "eyJhbGciOiJIUzI1NiJ9",
        "ghp_abcdefghijklmnopqrstuvwx",
        "github_pat_abcdefghijklmnopqrstuvwx",
        "sk-ant-abcdefghijklmnopqrstuvwx",
        "xoxb-abcdefghijklmnopqrstuvwx",
        "AKIAABCDEFGHIJKLMNOP",
    ] {
        assert!(!json.contains(secret), "{json}");
    }
    assert!(json.contains("request denied"));
}
