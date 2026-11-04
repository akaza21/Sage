use super::capability::{DevtoolsCapability, DevtoolsPort};
use super::diagnostics::DiagnosticRings;
use super::events::ingest;
use super::screenshot::admit_jpeg;
use super::snapshot::{DocumentStamp, NativeNode};
use super::wait::{WaitBudget, WaitStep};
use crate::acp::{
    browser::{
        BrowserAction, BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest,
        BrowserResult, BrowserScope, ViewportRegion, WaitCondition, dispatch,
    },
    limits::BROWSER_LIMITS,
};
use std::sync::{Arc, Mutex};

#[test]
fn suspended_diagnostics_drop_history_and_ignore_late_events() {
    let mut rings = DiagnosticRings::default();
    rings.push_console("log", "before sleep");
    rings.push_network("GET", "https://example.com", 200);
    rings.suspend();
    rings.push_console("log", "late event");
    rings.push_network("GET", "https://example.com/late", 200);
    assert!(rings.console(None, 10).unwrap().1.is_empty());
    assert!(rings.network(None, 10).unwrap().1.is_empty());
}

#[test]
fn diagnostic_ring_bounds_entries_and_rejects_a_lost_cursor() {
    let mut ring = DiagnosticRings::default();
    for index in 0..=BROWSER_LIMITS.diagnostic_entries {
        ring.push_console("log", &format!("entry-{index}"));
    }
    assert_eq!(ring.console(Some(0), 64), Err(BrowserFailure::Capacity));
    let (cursor, entries) = ring.console(Some(1), 64).unwrap();
    assert_eq!(entries.len(), BROWSER_LIMITS.diagnostic_entries);
    assert_eq!(cursor, BROWSER_LIMITS.diagnostic_entries as u64 + 1);
}

#[test]
fn diagnostics_are_redacted_before_they_leave_the_ring() {
    let mut ring = DiagnosticRings::default();
    ring.push_console("log", "Authorization: Bearer private-value");
    ring.push_network("GET", "https://site.test/?token=private-value", 200);
    let (_, console) = ring.console(None, 1).unwrap();
    let (_, network) = ring.network(None, 1).unwrap();
    assert!(!console[0].text.contains("private-value"));
    assert!(!network[0].url.contains("private-value"));
    assert!(!format!("{:?}", ring).contains("private-value"));
}

#[test]
fn cdp_network_events_keep_metadata_but_discard_cookies_and_bodies() {
    let rings = Arc::new(Mutex::new(DiagnosticRings::default()));
    ingest(
        &rings,
        "Network.requestWillBeSent",
        r#"{"request":{"method":"POST","url":"https://site.test/?token=private","headers":{"Cookie":"session=leak"},"postData":"body-secret"}}"#,
    );
    ingest(
        &rings,
        "Network.responseReceived",
        r#"{"response":{"url":"https://site.test/","status":200,"headers":{"Set-Cookie":"session=leak"},"body":"body-secret"}}"#,
    );
    let ring = rings.lock().unwrap();
    let (_, entries) = ring.network(None, 2).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].method, "POST");
    assert_eq!(entries[1].status, 200);
    let debug = format!("{ring:?}");
    for secret in ["private", "session=leak", "body-secret"] {
        assert!(!debug.contains(secret));
    }
}

#[test]
fn screenshot_rejects_oversized_or_non_jpeg_payloads() {
    assert!(admit_jpeg(640, 480, "/9j/AA==").is_ok());
    assert_eq!(
        admit_jpeg(BROWSER_LIMITS.screenshot_width + 1, 480, "/9j/AA=="),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(
        admit_jpeg(640, 480, "not-jpeg"),
        Err(BrowserFailure::InvalidRequest)
    );
}

#[test]
fn user_png_capture_steps_down_only_on_capacity_and_stops_after_three_attempts() {
    let mut scales = Vec::new();
    let result = super::screenshot::capture_png_with_fallback(|scale| {
        scales.push(scale);
        if scales.len() < 3 {
            Err(BrowserFailure::Capacity)
        } else {
            Ok("png")
        }
    });
    assert_eq!(result, Ok("png"));
    assert_eq!(scales, [1.0, 0.75, 0.5]);
    let mut attempts = 0;
    assert_eq!(
        super::screenshot::capture_png_with_fallback(|_| {
            attempts += 1;
            Err::<(), _>(BrowserFailure::Capacity)
        }),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(attempts, 3);
}

#[test]
fn user_png_admission_checks_signature_dimensions_and_encoded_cap() {
    let pixel = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+tmX8AAAAASUVORK5CYII=";
    assert_eq!(super::screenshot::admit_png(pixel), Ok((1, 1)));
    assert_eq!(
        super::screenshot::admit_png("/9j/AA=="),
        Err(BrowserFailure::InvalidRequest)
    );
    assert_eq!(
        super::screenshot::admit_png(&"A".repeat(BROWSER_LIMITS.screenshot_bytes + 1)),
        Err(BrowserFailure::Capacity)
    );
}

#[test]
fn wait_budget_bounds_probes_and_observes_cancellation() {
    let budget = WaitBudget::new(250).unwrap();
    assert_eq!(budget.step(0, false), WaitStep::Probe);
    assert_eq!(budget.step(100, false), WaitStep::Probe);
    assert_eq!(budget.step(249, false), WaitStep::Probe);
    assert_eq!(budget.step(250, false), WaitStep::TimedOut);
    assert_eq!(budget.step(10, true), WaitStep::Cancelled);
    assert_eq!(WaitBudget::new(0), Err(BrowserFailure::Capacity));
}

#[derive(Clone)]
struct FakePort;

impl DevtoolsPort for FakePort {
    fn navigate(&mut self, url: &str) -> Result<(String, u64), BrowserFailure> {
        Ok((url.to_owned(), 8))
    }
    fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure> {
        Ok((
            DocumentStamp {
                navigation_id: 7,
                generation: 1,
            },
            vec![NativeNode {
                backend_id: 10,
                depth: 1,
                role: "button".into(),
                name: "Continue token=private-value".into(),
            }],
        ))
    }
    fn act(
        &mut self,
        stamp: DocumentStamp,
        backend_id: u64,
        _action: &BrowserAction,
    ) -> Result<bool, BrowserFailure> {
        Ok(stamp.navigation_id == 7 && backend_id == 10)
    }
    fn capture(&mut self, region: Option<ViewportRegion>) -> Result<BrowserResult, BrowserFailure> {
        let region = region.ok_or(BrowserFailure::InvalidRequest)?;
        let jpeg_base64 = "/9j/AA==";
        admit_jpeg(region.width, region.height, jpeg_base64)?;
        Ok(BrowserResult::Screenshot {
            width: region.width,
            height: region.height,
            jpeg_base64: jpeg_base64.into(),
        })
    }

    fn matches(
        &mut self,
        condition: &WaitCondition,
        _remaining: std::time::Duration,
    ) -> Result<bool, BrowserFailure> {
        Ok(matches!(condition, WaitCondition::Text { text } if text == "ready"))
    }
}

#[test]
fn capability_routes_admitted_navigation_through_the_port() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let mut capability = DevtoolsCapability::new(scope.clone(), FakePort);
    let response = dispatch(
        &mut capability,
        BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::Navigate {
                url: "https://example.com/target".into(),
            },
        },
    )
    .unwrap();
    assert!(
        matches!(response.result, BrowserResult::Navigate { ref url, navigation_epoch: 8 } if url == "https://example.com/target")
    );
    let blocked = dispatch(
        &mut capability,
        BrowserRequest {
            scope,
            command: BrowserCommand::Navigate {
                url: "javascript:alert(1)".into(),
            },
        },
    );
    assert_eq!(blocked, Err(BrowserFailure::InvalidRequest));
}

#[test]
fn capability_seam_gates_explicit_capture_read_and_wait_for_a_fake_port() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let mut capability = DevtoolsCapability::new(scope.clone(), FakePort);
    let screenshot = dispatch(
        &mut capability,
        BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::Screenshot {
                region: Some(ViewportRegion {
                    x: 0,
                    y: 0,
                    width: 640,
                    height: 480,
                }),
            },
        },
    )
    .unwrap();
    assert!(matches!(
        screenshot.result,
        BrowserResult::Screenshot {
            width: 640,
            height: 480,
            ..
        }
    ));
    capability
        .diagnostics()
        .lock()
        .unwrap()
        .push_console("log", "token=private");
    let console = dispatch(
        &mut capability,
        BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::ReadConsole {
                cursor: None,
                limit: 1,
            },
        },
    )
    .unwrap();
    assert!(
        matches!(console.result, BrowserResult::ReadConsole { entries, .. } if entries.len() == 1 && !entries[0].text.contains("private"))
    );
    let wait = dispatch(
        &mut capability,
        BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::WaitFor {
                condition: WaitCondition::Text {
                    text: "ready".into(),
                },
                timeout_ms: 100,
            },
        },
    )
    .unwrap();
    assert_eq!(wait.result, BrowserResult::WaitFor { matched: true });
    capability.cancel();
    assert_eq!(
        capability.perform(&BrowserRequest {
            scope,
            command: BrowserCommand::ReadNetwork {
                cursor: None,
                limit: 1
            },
        }),
        Err(BrowserFailure::Denied)
    );
}

#[test]
fn snapshot_page_issues_bounded_redacted_document_handles() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let mut capability = DevtoolsCapability::new(scope.clone(), FakePort);
    let result = dispatch(
        &mut capability,
        BrowserRequest {
            scope,
            command: BrowserCommand::SnapshotPage {},
        },
    )
    .unwrap();
    let BrowserResult::SnapshotPage { nodes, text, .. } = result.result else {
        panic!("expected snapshot");
    };
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].role, "button");
    assert_eq!(nodes[0].name, "Continue token=[redacted]");
    assert!(!text.contains("private-value"));
}

#[test]
fn action_uses_the_document_handle_from_the_latest_snapshot() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let mut capability = DevtoolsCapability::new(scope.clone(), FakePort);
    let snapshot = capability
        .perform(&BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::SnapshotPage {},
        })
        .unwrap();
    let BrowserResult::SnapshotPage { nodes, .. } = snapshot else {
        panic!("expected snapshot")
    };
    let result = capability.perform(&BrowserRequest {
        scope,
        command: BrowserCommand::Act {
            action: BrowserAction::Click {
                handle: nodes[0].handle.clone(),
            },
        },
    });
    assert_eq!(result, Ok(BrowserResult::Act { applied: true }));
}

#[test]
fn action_rejects_replaced_and_foreign_snapshot_handles() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let mut first = DevtoolsCapability::new(scope.clone(), FakePort);
    let BrowserResult::SnapshotPage { nodes: old, .. } = first
        .perform(&BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::SnapshotPage {},
        })
        .unwrap()
    else {
        panic!("expected snapshot")
    };
    let mut other = DevtoolsCapability::new(scope.clone(), FakePort);
    let BrowserResult::SnapshotPage { nodes: foreign, .. } = other
        .perform(&BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::SnapshotPage {},
        })
        .unwrap()
    else {
        panic!("expected snapshot")
    };
    let BrowserResult::SnapshotPage { .. } = first
        .perform(&BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::SnapshotPage {},
        })
        .unwrap()
    else {
        panic!("expected snapshot")
    };
    for handle in [old[0].handle.clone(), foreign[0].handle.clone()] {
        assert_eq!(
            first.perform(&BrowserRequest {
                scope: scope.clone(),
                command: BrowserCommand::Act {
                    action: BrowserAction::Click { handle }
                }
            }),
            Err(BrowserFailure::StaleHandle)
        );
    }
}

#[test]
fn snapshot_handles_survive_a_dispatch_clone() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let mut cached = DevtoolsCapability::new(scope.clone(), FakePort);
    let mut dispatched = cached.clone();
    let BrowserResult::SnapshotPage { nodes, .. } = dispatched
        .perform(&BrowserRequest {
            scope: scope.clone(),
            command: BrowserCommand::SnapshotPage {},
        })
        .unwrap()
    else {
        panic!("expected snapshot")
    };
    assert_eq!(
        cached.perform(&BrowserRequest {
            scope,
            command: BrowserCommand::Act {
                action: BrowserAction::Click {
                    handle: nodes[0].handle.clone()
                }
            },
        }),
        Ok(BrowserResult::Act { applied: true })
    );
}

/// A page that never shows the text and whose dispatch the user stops
/// after `stop_after` probes.
#[derive(Clone)]
struct StoppedWhileWaiting {
    probes: Arc<Mutex<u32>>,
    stop_after: u32,
}

impl DevtoolsPort for StoppedWhileWaiting {
    fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }
    fn capture(&mut self, _: Option<ViewportRegion>) -> Result<BrowserResult, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }
    fn matches(
        &mut self,
        _: &WaitCondition,
        _: std::time::Duration,
    ) -> Result<bool, BrowserFailure> {
        *self.probes.lock().unwrap() += 1;
        Ok(false)
    }
    fn stopped(&self) -> bool {
        *self.probes.lock().unwrap() >= self.stop_after
    }
}

#[test]
fn a_wait_stops_probing_the_page_once_the_user_stops_or_takes_over() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    let probes = Arc::new(Mutex::new(0));
    let mut capability = DevtoolsCapability::new(
        scope.clone(),
        StoppedWhileWaiting {
            probes: probes.clone(),
            stop_after: 2,
        },
    );
    let started = std::time::Instant::now();
    let wait = capability.perform(&BrowserRequest {
        scope,
        command: BrowserCommand::WaitFor {
            condition: WaitCondition::Text {
                text: "ready".into(),
            },
            timeout_ms: 5_000,
        },
    });
    assert_eq!(wait, Err(BrowserFailure::Denied));
    assert_eq!(*probes.lock().unwrap(), 2);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

mod developer_objects;
mod handle_invalidation;
