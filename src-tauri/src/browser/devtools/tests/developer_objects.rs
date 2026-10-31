//! Developer-mode evaluations never leave results pinned in the page
//! (ADR-0009b): by value, in Sage's group, released after every call.
use super::*;
use crate::acp::browser::developer::{DEVELOPER_OBJECT_GROUP, EVALUATE_TIMEOUT_MS};
use std::time::Duration;

#[derive(Clone, Default)]
struct RecordingPort {
    raw: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    releases: Arc<Mutex<usize>>,
    fail: Option<BrowserFailure>,
}

impl DevtoolsPort for RecordingPort {
    fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }
    fn act(&mut self, _: DocumentStamp, _: u64, _: &BrowserAction) -> Result<bool, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }
    fn capture(&mut self, _: Option<ViewportRegion>) -> Result<BrowserResult, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }
    fn matches(&mut self, _: &WaitCondition, _: Duration) -> Result<bool, BrowserFailure> {
        Ok(false)
    }
    fn raw(&mut self, method: &str, params: &str) -> Result<String, BrowserFailure> {
        self.raw
            .lock()
            .unwrap()
            .push((method.into(), serde_json::from_str(params).unwrap()));
        match self.fail {
            Some(failure) => Err(failure),
            None => Ok(r#"{"result":{"type":"number","value":2}}"#.into()),
        }
    }
    fn release_developer_objects(&mut self) {
        *self.releases.lock().unwrap() += 1;
    }
}

fn evaluate(scope: &BrowserScope) -> BrowserRequest {
    evaluate_with(
        scope,
        serde_json::json!({"expression": "new ArrayBuffer(8)"}),
    )
}

fn evaluate_with(scope: &BrowserScope, params: serde_json::Value) -> BrowserRequest {
    BrowserRequest {
        scope: scope.clone(),
        command: BrowserCommand::Cdp {
            method: "Runtime.evaluate".into(),
            params: params.as_object().cloned().unwrap(),
        },
    }
}

#[test]
fn an_evaluation_runs_by_value_in_sages_group_and_is_released_whatever_happens() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    // Revoked mid-call (Stop, Take over, switch off) or failed: still released.
    for fail in [
        None,
        Some(BrowserFailure::Denied),
        Some(BrowserFailure::OutcomeUnknown),
    ] {
        let port = RecordingPort {
            fail,
            ..RecordingPort::default()
        };
        let mut capability = DevtoolsCapability::new(scope.clone(), port.clone());
        let outcome = capability.perform(&evaluate(&scope));
        assert_eq!(outcome.is_ok(), fail.is_none(), "{fail:?}");
        let raw = port.raw.lock().unwrap();
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].0, "Runtime.evaluate");
        assert_eq!(raw[0].1["returnByValue"], true);
        assert_eq!(raw[0].1["objectGroup"], DEVELOPER_OBJECT_GROUP);
        assert_eq!(raw[0].1["timeout"], EVALUATE_TIMEOUT_MS);
        assert_eq!(*port.releases.lock().unwrap(), 1, "{fail:?}");
    }
}

#[test]
fn an_evaluation_never_runs_longer_than_the_bound() {
    let scope = BrowserScope {
        session_id: "session-1".into(),
        tab_id: 1,
    };
    for (asked, sent) in [
        (100, 100),
        (EVALUATE_TIMEOUT_MS, EVALUATE_TIMEOUT_MS),
        (EVALUATE_TIMEOUT_MS * 40, EVALUATE_TIMEOUT_MS),
    ] {
        let port = RecordingPort::default();
        let mut capability = DevtoolsCapability::new(scope.clone(), port.clone());
        let request = evaluate_with(
            &scope,
            serde_json::json!({"expression": "1", "timeout": asked}),
        );
        capability.perform(&request).unwrap();
        assert_eq!(port.raw.lock().unwrap()[0].1["timeout"], sent, "{asked}");
    }
}

#[test]
fn releasing_goes_to_the_page_outside_the_permit() {
    struct Observed(Mutex<Vec<(String, String)>>);
    impl crate::browser::devtools::transport::CdpTransport for Observed {
        fn call(&self, _: &str, _: &str, _: usize, _: Duration) -> Result<String, BrowserFailure> {
            Err(BrowserFailure::Denied)
        }
        fn observe(
            &self,
            method: &str,
            params: &str,
            _: usize,
            _: Duration,
        ) -> Result<String, BrowserFailure> {
            self.0.lock().unwrap().push((method.into(), params.into()));
            Ok("{}".into())
        }
        fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
            Err(BrowserFailure::Unavailable)
        }
        fn issued(&self) -> bool {
            false
        }
        fn settled(
            &self,
            _: DocumentStamp,
        ) -> Result<Option<crate::browser::devtools::transport::Settled>, BrowserFailure> {
            Ok(None)
        }
    }
    let transport = Observed(Mutex::default());
    super::super::cdp_port::release_developer_objects(&transport);
    let sent = transport.0.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "Runtime.releaseObjectGroup");
    assert!(sent[0].1.contains(DEVELOPER_OBJECT_GROUP));
}
