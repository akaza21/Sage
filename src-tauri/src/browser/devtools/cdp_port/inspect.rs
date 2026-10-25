//! The user's Annotate mode: Chromium's own inspect overlay outlines the
//! element under the pointer. Nothing runs in the page; every step is a
//! permitless read outside any Agent dispatch.

use serde_json::json;
use std::time::Duration;

use crate::acp::browser::BrowserFailure;

use super::{CdpPort, CdpTransport};

const STEP_TIMEOUT: Duration = Duration::from_secs(2);
const RESULT_BYTES: usize = 4096;

/// The Annotate blue (`--chat-annotation-accent`, #3a83f7).
const BLUE: (u8, u8, u8) = (58, 131, 247);

/// Steps that leave inspect mode, in order. Each runs even if one before it
/// failed, so the page never keeps a half-enabled overlay.
const STOP: [(&str, &str); 4] = [
    ("Overlay.setInspectMode", r#"{"mode":"none","highlightConfig":{}}"#),
    ("Overlay.hideHighlight", "{}"),
    ("Overlay.disable", "{}"),
    ("DOM.disable", "{}"),
];

impl<T: CdpTransport> CdpPort<T> {
    /// Turns on hover highlighting. A failed step leaves inspect mode again.
    pub(crate) fn start_inspect(&self) -> Result<(), BrowserFailure> {
        let started = ["DOM.enable", "Overlay.enable"]
            .into_iter()
            .try_for_each(|method| self.inspect_step(method, "{}"))
            .and_then(|()| self.inspect_step("Overlay.setInspectMode", &inspect_params()));
        if started.is_err() {
            let _ = self.stop_inspect();
        }
        started
    }

    /// Leaves inspect mode and clears any highlight; reports the first
    /// failure after trying every step.
    pub(crate) fn stop_inspect(&self) -> Result<(), BrowserFailure> {
        STOP.into_iter()
            .map(|(method, params)| self.inspect_step(method, params))
            .fold(Ok(()), Result::and)
    }

    fn inspect_step(&self, method: &str, params: &str) -> Result<(), BrowserFailure> {
        self.transport
            .observe(method, params, RESULT_BYTES, STEP_TIMEOUT)
            .map(drop)
    }
}

/// `searchForNode` with a blue box and no info tooltip, rulers or styles.
/// CDP has no outline colour, so the content and padding boxes get a light
/// tint and the CSS border a solid edge.
pub(super) fn inspect_params() -> String {
    let (r, g, b) = BLUE;
    json!({
        "mode": "searchForNode",
        "highlightConfig": {
            "showInfo": false,
            "showStyles": false,
            "showRulers": false,
            "showAccessibilityInfo": false,
            "showExtensionLines": false,
            "contentColor": {"r": r, "g": g, "b": b, "a": 0.16},
            "paddingColor": {"r": r, "g": g, "b": b, "a": 0.16},
            "borderColor": {"r": r, "g": g, "b": b, "a": 1.0},
            "marginColor": {"r": 0, "g": 0, "b": 0, "a": 0.0}
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::browser::devtools::{snapshot::DocumentStamp, transport::Settled};

    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<(String, String, bool)>>,
        fail: Option<&'static str>,
    }

    impl Recorder {
        fn methods(&self) -> Vec<String> {
            let calls = self.calls.lock().unwrap();
            calls.iter().map(|(method, _, _)| method.clone()).collect()
        }
        fn record(&self, method: &str, params: &str, permitted: bool) -> Result<String, BrowserFailure> {
            self.calls
                .lock()
                .unwrap()
                .push((method.into(), params.into(), permitted));
            if self.fail == Some(method) {
                return Err(BrowserFailure::Unavailable);
            }
            Ok("{}".into())
        }
    }

    impl CdpTransport for Recorder {
        fn call(&self, method: &str, params: &str, _: usize, _: Duration) -> Result<String, BrowserFailure> {
            self.record(method, params, true)
        }
        fn observe(&self, method: &str, params: &str, _: usize, _: Duration) -> Result<String, BrowserFailure> {
            self.record(method, params, false)
        }
        fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
            Err(BrowserFailure::Unavailable)
        }
        fn issued(&self) -> bool {
            false
        }
        fn settled(&self, _: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
            Ok(None)
        }
    }

    fn port(fail: Option<&'static str>) -> CdpPort<Recorder> {
        CdpPort::new(Recorder {
            fail,
            ..Recorder::default()
        })
    }

    #[test]
    fn start_enables_dom_and_overlay_then_searches_for_nodes_without_the_agent_permit() {
        let port = port(None);
        assert_eq!(port.start_inspect(), Ok(()));
        assert_eq!(
            port.transport.methods(),
            ["DOM.enable", "Overlay.enable", "Overlay.setInspectMode"]
        );
        let calls = port.transport.calls.lock().unwrap();
        assert!(calls.iter().all(|(_, _, permitted)| !permitted));
        let params: serde_json::Value = serde_json::from_str(&calls[2].1).unwrap();
        assert_eq!(params["mode"], "searchForNode");
        let config = &params["highlightConfig"];
        assert_eq!(config["showInfo"], false);
        assert_eq!(config["borderColor"], json!({"r": 58, "g": 131, "b": 247, "a": 1.0}));
        assert_eq!(config["contentColor"]["b"], 247);
        assert_eq!(config["marginColor"]["a"], 0.0);
    }

    #[test]
    fn a_failed_start_step_leaves_inspect_mode_again() {
        let port = port(Some("Overlay.setInspectMode"));
        assert_eq!(port.start_inspect(), Err(BrowserFailure::Unavailable));
        let methods = port.transport.methods();
        assert_eq!(&methods[..3], ["DOM.enable", "Overlay.enable", "Overlay.setInspectMode"]);
        assert_eq!(&methods[3..], STOP.map(|(method, _)| method));
    }

    #[test]
    fn stop_tries_every_step_and_reports_the_first_failure() {
        let port = port(Some("Overlay.hideHighlight"));
        assert_eq!(port.stop_inspect(), Err(BrowserFailure::Unavailable));
        assert_eq!(port.transport.methods(), STOP.map(|(method, _)| method));
        let calls = port.transport.calls.lock().unwrap();
        assert_eq!(calls[0].1, r#"{"mode":"none","highlightConfig":{}}"#);
        assert!(calls.iter().all(|(_, _, permitted)| !permitted));
    }
}
