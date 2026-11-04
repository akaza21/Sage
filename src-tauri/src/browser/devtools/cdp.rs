use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    time::Duration,
};

use tauri::{Manager, Webview};
use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use windows_core::PCWSTR;

use crate::acp::browser::BrowserFailure;
use crate::acp::browser::permission::BrowserDispatchPermit;

use super::super::BrowserState;

const QUEUED: u8 = 0;
const ISSUED: u8 = 1;
const CANCELLED: u8 = 2;

#[derive(Clone)]
struct IssueAttempt(Arc<AtomicU8>);

impl IssueAttempt {
    fn new() -> Self {
        Self(Arc::new(AtomicU8::new(QUEUED)))
    }
    fn issue<T>(
        &self,
        issued_marker: Option<&AtomicBool>,
        issue: impl FnOnce() -> Result<T, BrowserFailure>,
    ) -> Result<T, BrowserFailure> {
        self.0
            .compare_exchange(QUEUED, ISSUED, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| BrowserFailure::Unavailable)?;
        if let Some(marker) = issued_marker {
            marker.store(true, Ordering::Release);
        }
        issue()
    }
    fn timeout(&self) -> BrowserFailure {
        match self
            .0
            .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) | Err(CANCELLED) => BrowserFailure::Unavailable,
            Err(_) => BrowserFailure::OutcomeUnknown,
        }
    }
    fn disconnected_failure(&self) -> BrowserFailure {
        self.timeout()
    }
}

pub(super) fn call(
    webview: &Webview,
    method: &str,
    params: &str,
    max_result_bytes: usize,
    timeout: Duration,
) -> Result<String, BrowserFailure> {
    call_with_permit(
        webview,
        method,
        params,
        max_result_bytes,
        timeout,
        None,
        None,
    )
}

pub(super) fn call_with_permit(
    webview: &Webview,
    method: &str,
    params: &str,
    max_result_bytes: usize,
    timeout: Duration,
    permit: Option<BrowserDispatchPermit>,
    issued_marker: Option<Arc<AtomicBool>>,
) -> Result<String, BrowserFailure> {
    if method.len() > 128 || params.len() > 8192 {
        return Err(BrowserFailure::Capacity);
    }
    let awake = Arc::new(
        crate::browser::background::ensure_awake(webview)
            .map_err(|_| BrowserFailure::Unavailable)?,
    );
    let method = wide(method);
    let params = wide(params);
    let attempt = IssueAttempt::new();
    let callback_attempt = attempt.clone();
    let timeout_permit = permit.clone();
    let app = webview.app_handle().clone();
    let tab_id = webview.label().to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    let completion_awake = awake.clone();
    webview
        .with_webview(move |platform| {
            let result = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|_| BrowserFailure::Unavailable)?;
                let completed_sender = sender.clone();
                let completed = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                    move |outcome, json| {
                        let _pin = &completion_awake;
                        let _ = completed_sender.try_send(if json.len() > max_result_bytes {
                            Err(BrowserFailure::Capacity)
                        } else if outcome.is_ok() {
                            Ok(json)
                        } else {
                            Err(BrowserFailure::Unavailable)
                        });
                        Ok(())
                    },
                ));
                let issue = || {
                    unsafe {
                        core.CallDevToolsProtocolMethod(
                            PCWSTR(method.as_ptr()),
                            PCWSTR(params.as_ptr()),
                            &completed,
                        )
                    }
                    .map_err(|_| BrowserFailure::Unavailable)
                };
                match permit {
                    Some(permit) => permit.with_live_lock(|| {
                        app.state::<BrowserState>()
                            .mark_agent_download_provenance(&tab_id);
                        callback_attempt.issue(issued_marker.as_deref(), || {
                            permit.mark_issued();
                            issue()
                        })
                    }),
                    None => callback_attempt.issue(issued_marker.as_deref(), issue),
                }
            })();
            if let Err(error) = result {
                let _ = sender.try_send(Err(error));
            }
        })
        .map_err(|_| BrowserFailure::Unavailable)?;
    let deadline = std::time::Instant::now() + timeout;
    let result = loop {
        if timeout_permit
            .as_ref()
            .is_some_and(|permit| !permit.is_live() && !permit.was_invalidated_by_navigation())
        {
            attempt.timeout();
            return Err(timeout_permit.as_ref().unwrap().revoked_failure());
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            let error = attempt.timeout();
            if error == BrowserFailure::OutcomeUnknown
                && let Some(permit) = timeout_permit
            {
                permit.revoke();
            }
            return Err(error);
        }
        match receiver.recv_timeout(remaining.min(Duration::from_millis(25))) {
            Ok(result) => break result?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(attempt.disconnected_failure());
            }
        }
    };
    if result.len() > max_result_bytes {
        return Err(BrowserFailure::Capacity);
    }
    Ok(result)
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
#[path = "cdp/tests.rs"]
mod tests;
