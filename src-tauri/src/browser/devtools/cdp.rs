use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc,
    },
    time::Duration,
};

use super::developer_session::Session;
use tauri::{Manager, Webview};
use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_11;
use windows_core::{Interface, PCWSTR};

#[derive(Default)]
pub(super) struct Dispatch {
    pub issued: Option<Arc<AtomicBool>>,
    pub session: Option<Arc<Session>>,
}

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
        Dispatch::default(),
    )
}

pub(super) fn call_with_permit(
    webview: &Webview,
    method: &str,
    params: &str,
    max_result_bytes: usize,
    timeout: Duration,
    permit: Option<BrowserDispatchPermit>,
    dispatch: Dispatch,
) -> Result<String, BrowserFailure> {
    // Developer-mode params are admitted up to `cdp_params_bytes` of JSON.
    if method.len() > 128 || params.len() > 16_384 {
        return Err(BrowserFailure::Capacity);
    }
    // A developer-mode evaluation releases its object group from the
    // completion callback itself, so a result or exception that arrives
    // after Sage stopped waiting (revoke, timeout) is not left pinned.
    let release_on_completion =
        crate::acp::browser::developer::releases_on_completion(method, params);
    let method = wide(method);
    let params = wide(params);
    let attempt = IssueAttempt::new();
    let callback_attempt = attempt.clone();
    let timeout_permit = permit.clone();
    let scoped_session = dispatch.session.clone();
    let app = webview.app_handle().clone();
    let tab_id = webview.label().to_owned();
    let (sender, receiver) = mpsc::sync_channel(1);
    webview
        .with_webview(move |platform| {
            let result = (|| {
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|_| BrowserFailure::Unavailable)?;
                let completed_sender = sender.clone();
                let release_core = release_on_completion.then(|| core.clone());
                let release_session = dispatch.session.clone();
                let completed = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                    move |outcome, json| {
                        let _ = completed_sender.try_send(if json.len() > max_result_bytes {
                            Err(BrowserFailure::Capacity)
                        } else if outcome.is_ok() {
                            Ok(json)
                        } else {
                            Err(BrowserFailure::Unavailable)
                        });
                        if let Some(core) = release_core {
                            if let Some(session) = release_session {
                                let _ = session.with_id(|id| {
                                    notify_session(
                                        &core,
                                        id,
                                        "Runtime.releaseObjectGroup",
                                        &crate::acp::browser::developer::release_params(),
                                    );
                                    Ok(())
                                });
                            } else {
                                release_developer_objects(&core);
                            }
                        }
                        Ok(())
                    },
                ));
                let issue = || {
                    if let Some(session) = &dispatch.session {
                        return session.with_id(|id| {
                            let session = wide(id);
                            unsafe {
                                core.cast::<ICoreWebView2_11>().and_then(|core| {
                                    core.CallDevToolsProtocolMethodForSession(
                                        PCWSTR(session.as_ptr()),
                                        PCWSTR(method.as_ptr()),
                                        PCWSTR(params.as_ptr()),
                                        &completed,
                                    )
                                })
                            }
                            .map_err(|_| BrowserFailure::Unavailable)
                        });
                    }
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
                        callback_attempt.issue(dispatch.issued.as_deref(), || {
                            permit.mark_issued();
                            issue()
                        })
                    }),
                    None => callback_attempt.issue(dispatch.issued.as_deref(), issue),
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
            if let Some(session) = &scoped_session {
                session.close(true);
            }
            return Err(timeout_permit.as_ref().unwrap().revoked_failure());
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            let error = attempt.timeout();
            if let Some(session) = &scoped_session {
                session.close(true);
            } else if error == BrowserFailure::OutcomeUnknown
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
                if let Some(session) = &scoped_session {
                    session.close(true);
                }
                return Err(attempt.disconnected_failure());
            }
        }
    };
    if result.len() > max_result_bytes {
        return Err(BrowserFailure::Capacity);
    }
    Ok(result)
}

/// Fire-and-forget `Runtime.releaseObjectGroup` on developer mode's group,
/// from the UI thread, outside any permit.
pub(super) fn release_developer_objects(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
) {
    notify(
        core,
        "Runtime.releaseObjectGroup",
        &crate::acp::browser::developer::release_params(),
    );
}

pub(super) fn notify(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    method: &str,
    params: &str,
) {
    let method = wide(method);
    let params = wide(params);
    let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
    let _ = unsafe {
        core.CallDevToolsProtocolMethod(PCWSTR(method.as_ptr()), PCWSTR(params.as_ptr()), &done)
    };
}

pub(super) fn notify_session(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    session: &str,
    method: &str,
    params: &str,
) {
    if session.is_empty() {
        return;
    }
    let session = wide(session);
    let method = wide(method);
    let params = wide(params);
    let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(())));
    if let Ok(core) = core.cast::<ICoreWebView2_11>() {
        let _ = unsafe {
            core.CallDevToolsProtocolMethodForSession(
                PCWSTR(session.as_ptr()),
                PCWSTR(method.as_ptr()),
                PCWSTR(params.as_ptr()),
                &done,
            )
        };
    }
}

pub(super) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
#[path = "cdp/tests.rs"]
mod tests;
