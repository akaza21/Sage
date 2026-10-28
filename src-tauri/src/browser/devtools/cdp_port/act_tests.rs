//! The act marker is best effort: it rides the permitless path, so a marker
//! step that times out after it was issued can never revoke the Agent's
//! permit (`cdp.rs` revokes only on the permitted path), and it never blocks
//! or fails the act it marks.

use serde_json::json;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use super::*;
use crate::{
    acp::browser::ElementHandle,
    browser::devtools::transport::{CdpTransport, Settled},
};

const STAMP: DocumentStamp = DocumentStamp {
    navigation_id: 3,
    generation: 5,
};

#[derive(Default)]
struct Page {
    /// Every command, with whether it rode the dispatch permit.
    log: Mutex<Vec<(bool, String)>>,
    /// Every marker step times out after it was issued.
    marker_times_out: bool,
    stopped: AtomicBool,
    annotating: bool,
}

impl Page {
    fn sent(&self, permitted: bool) -> Vec<String> {
        let log = self.log.lock().unwrap();
        log.iter()
            .filter(|(by_permit, _)| *by_permit == permitted)
            .map(|(_, method)| method.clone())
            .collect()
    }
}

impl CdpTransport for Page {
    fn call(&self, method: &str, _: &str, _: usize, _: Duration) -> Result<String, BrowserFailure> {
        self.log.lock().unwrap().push((true, method.to_owned()));
        Ok("{}".into())
    }

    fn observe(
        &self,
        method: &str,
        _: &str,
        _: usize,
        _: Duration,
    ) -> Result<String, BrowserFailure> {
        self.log.lock().unwrap().push((false, method.to_owned()));
        if self.marker_times_out {
            return Err(BrowserFailure::OutcomeUnknown);
        }
        Ok(match method {
            "DOM.getBoxModel" => json!({"model": {"border": [0, 0, 20, 0, 20, 20, 0, 20]}}),
            _ => json!({}),
        }
        .to_string())
    }

    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
        Ok(STAMP)
    }

    fn issued(&self) -> bool {
        true
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    fn annotating(&self) -> bool {
        self.annotating
    }

    fn settled(&self, _: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
        unreachable!()
    }
}

fn press(page: Page) -> (Result<bool, BrowserFailure>, CdpPort<Page>) {
    let mut port = CdpPort::new(page);
    let key = BrowserAction::Key {
        handle: ElementHandle::from_token("h".into()),
        key: "a".into(),
    };
    (perform(&mut port, STAMP, 9, &key), port)
}

const MARK: [&str; 5] = [
    "DOM.enable",
    "Overlay.enable",
    "Overlay.highlightNode",
    "DOM.getBoxModel",
    "Overlay.setShowHinge",
];

#[test]
fn a_marker_that_times_out_never_rides_the_permit_and_the_act_still_runs() {
    let (result, port) = press(Page {
        marker_times_out: true,
        ..Page::default()
    });
    assert_eq!(result, Ok(true));
    assert_eq!(
        port.transport.sent(true),
        [
            "DOM.focus",
            "Input.dispatchKeyEvent",
            "Input.dispatchKeyEvent"
        ]
    );
    // The first marker step failed, so the marker was cleared at once and
    // the act ran without waiting for it.
    assert_eq!(
        port.transport.sent(false),
        [
            "DOM.enable",
            "Overlay.hideHighlight",
            "Overlay.setShowHinge",
            "Overlay.disable",
            "DOM.disable",
        ]
    );
}

#[test]
fn the_marker_shows_before_the_act_and_is_cleared_after_it() {
    let (result, port) = press(Page::default());
    assert_eq!(result, Ok(true));
    let log = port.transport.log.lock().unwrap().clone();
    let methods: Vec<&str> = log.iter().map(|(_, method)| method.as_str()).collect();
    assert_eq!(
        methods,
        [
            MARK[0],
            MARK[1],
            MARK[2],
            MARK[3],
            MARK[4],
            "DOM.focus",
            "Input.dispatchKeyEvent",
            "Input.dispatchKeyEvent",
            "Overlay.hideHighlight",
            "Overlay.setShowHinge",
            "Overlay.disable",
            "DOM.disable",
        ]
    );
    assert!(log.iter().all(|(permitted, method)| *permitted
        == (method == "DOM.focus" || method == "Input.dispatchKeyEvent")));
}

#[test]
fn while_the_tab_annotates_the_marker_leaves_annotates_outline_and_overlay_alone() {
    let (_, port) = press(Page {
        annotating: true,
        ..Page::default()
    });
    // The hinge is Annotate's hover outline (BP-H2): the marker neither
    // draws nor clears it, and the inspect overlay keeps both domains on.
    assert_eq!(
        port.transport.sent(false),
        [MARK[0], MARK[1], MARK[2], "Overlay.hideHighlight"]
    );
}

#[test]
fn a_stopped_dispatch_draws_no_marker() {
    let page = Page::default();
    page.stopped.store(true, Ordering::SeqCst);
    let (_, port) = press(page);
    assert!(port.transport.sent(false).is_empty());
}
