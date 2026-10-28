use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{Manager, Webview};

use crate::acp::browser::{
    BrowserCapability, BrowserFailure, BrowserRequest, BrowserScope,
    permission::{BrowserCapabilityDispatch, BrowserDispatchPermit, PendingBrowserResult},
};

use super::{
    capability::DevtoolsCapability,
    cdp,
    cdp_port::{
        CdpPort,
        outcome::{dispatch_disconnected_failure, dispatch_timeout_failure},
        page_context::{self, CapturedPageContext},
        select::PickedNode,
    },
    events,
    snapshot::DocumentStamp,
    transport::{CdpTransport, Settled},
};
use crate::browser::BrowserState;

pub(crate) fn capture_user_png(
    state: &BrowserState,
    tab_id: &str,
    expected_revision: &str,
) -> Result<(u32, u32, String), String> {
    let webview = state
        .tabs
        .lock()
        .map_err(|_| "browser unavailable")?
        .handle(tab_id)
        .cloned()
        .ok_or("browser tab unavailable")?;
    NativePort::new(NativeTransport {
        webview,
        permit: None,
        issued: Arc::new(AtomicBool::new(false)),
    })
    .capture_user_png(expected_revision)
}

pub(crate) fn capture_page_context(
    state: &BrowserState,
    tab_id: &str,
    expected_revision: &str,
) -> Result<CapturedPageContext, String> {
    let webview = state
        .tabs
        .lock()
        .map_err(|_| "browser unavailable")?
        .handle(tab_id)
        .cloned()
        .ok_or("browser tab unavailable")?;
    let mut port = NativePort::new(NativeTransport {
        webview,
        permit: None,
        issued: Arc::new(AtomicBool::new(false)),
    });
    page_context::capture_page_context(&mut port, tab_id, expected_revision, || {
        let tabs = state.tabs.lock().map_err(|_| "browser unavailable")?;
        tabs.openable_url(tab_id)
            .map_err(|_| "browser page unavailable".to_owned())
    })
}

pub(crate) fn clear_agent_highlight(webview: &Webview) -> Result<(), BrowserFailure> {
    cdp::call(
        webview,
        "Overlay.hideHighlight",
        "{}",
        4096,
        Duration::from_secs(1),
    )?;
    Ok(())
}

/// Turns the user's Annotate hover highlight on or off, outside any Agent
/// permit.
pub(crate) fn set_annotate(webview: &Webview, on: bool) -> Result<(), BrowserFailure> {
    let port = NativePort::new(NativeTransport {
        webview: webview.clone(),
        permit: None,
        issued: Arc::new(AtomicBool::new(false)),
    });
    if on {
        port.start_inspect()
    } else {
        port.stop_inspect()
    }
}

/// Reads the element the user clicked in Annotate and freezes its frame.
pub(crate) fn pick_element(webview: &Webview, backend_node_id: u64) -> Result<PickedNode, String> {
    NativePort::new(NativeTransport {
        webview: webview.clone(),
        permit: None,
        issued: Arc::new(AtomicBool::new(false)),
    })
    .pick_element(backend_node_id)
    .map_err(|error| match error {
        BrowserFailure::StaleHandle => "browser page changed; pick the element again".into(),
        _ => "browser element unavailable".into(),
    })
}

pub(crate) type NativePort = CdpPort<NativeTransport>;
pub(crate) type NativeDevtools = DevtoolsCapability<NativePort>;

impl BrowserCapabilityDispatch for &mut NativeDevtools {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        let mut capability = (**self).clone();
        let timeout_permit = permit.clone();
        let issued = Arc::new(AtomicBool::new(false));
        capability.port_mut().transport.issued = issued.clone();
        capability.port_mut().transport.permit = Some(permit);
        let request = request.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(capability.perform(&request));
        });
        Ok(Box::new(move || {
            let deadline = Instant::now() + Duration::from_secs(35);
            loop {
                if !timeout_permit.is_live() && !timeout_permit.was_invalidated_by_navigation() {
                    return Err(timeout_permit.revoked_failure());
                }
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    timeout_permit.revoke();
                    return Err(dispatch_timeout_failure(issued.load(Ordering::Acquire)));
                }
                match receiver.recv_timeout(remaining.min(Duration::from_millis(25))) {
                    Ok(result) => return result,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(dispatch_disconnected_failure(
                            issued.load(Ordering::Acquire),
                        ));
                    }
                }
            }
        }))
    }
}

/// Carries Agent CDP commands to one WebView2 browser tab.
#[derive(Clone)]
pub(crate) struct NativeTransport {
    webview: Webview,
    permit: Option<BrowserDispatchPermit>,
    issued: Arc<AtomicBool>,
}

impl CdpTransport for NativeTransport {
    fn call(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        cdp::call_with_permit(
            &self.webview,
            method,
            params,
            max_result_bytes,
            timeout,
            self.permit.clone(),
            Some(self.issued.clone()),
        )
    }

    fn observe(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        cdp::call(&self.webview, method, params, max_result_bytes, timeout)
    }

    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
        let app = self.webview.app_handle();
        let state = app.state::<BrowserState>();
        let tabs = state.tabs.lock().map_err(|_| BrowserFailure::Unavailable)?;
        let tab = tabs
            .snapshot(self.webview.label())
            .ok_or(BrowserFailure::StaleHandle)?;
        if tab.loading || tab.error.is_some() || tab.committed_url.is_none() {
            return Err(BrowserFailure::Unavailable);
        }
        Ok(DocumentStamp {
            navigation_id: tab.navigation_id.unwrap_or(0),
            generation: tab.revision,
        })
    }

    fn issued(&self) -> bool {
        self.issued.load(Ordering::Acquire)
    }

    fn settled(&self, before: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
        if self
            .permit
            .as_ref()
            .is_some_and(|permit| !permit.is_live() && !permit.was_invalidated_by_navigation())
        {
            return Err(self.permit.as_ref().unwrap().revoked_failure());
        }
        let state = self.webview.app_handle().state::<BrowserState>();
        let snapshot = state
            .tabs
            .lock()
            .map_err(|_| BrowserFailure::OutcomeUnknown)?
            .snapshot(self.webview.label())
            .ok_or(BrowserFailure::OutcomeUnknown)?;
        if snapshot.revision > before.generation && !snapshot.loading {
            if snapshot.error.is_some() {
                return Err(BrowserFailure::OutcomeUnknown);
            }
            if let Some(committed_url) = snapshot.committed_url {
                return Ok(Some(Settled {
                    url: committed_url,
                    navigation_id: snapshot.navigation_id.unwrap_or(0),
                }));
            }
        }
        Ok(None)
    }
}

impl DevtoolsCapability<NativePort> {
    pub(crate) fn for_tab(
        state: &BrowserState,
        scope: BrowserScope,
    ) -> Result<Self, BrowserFailure> {
        let label = format!("browser-{}", scope.tab_id);
        let webview = state
            .tabs
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .handle(&label)
            .cloned()
            .ok_or(BrowserFailure::Unavailable)?;
        Self::from_webview(scope, webview)
    }

    fn from_webview(scope: BrowserScope, webview: Webview) -> Result<Self, BrowserFailure> {
        if webview.label() != format!("browser-{}", scope.tab_id) {
            return Err(BrowserFailure::Denied);
        }
        let diagnostics = events::attach(&webview)?;
        Ok(Self::with_diagnostics(
            scope,
            NativePort::new(NativeTransport {
                webview,
                permit: None,
                issued: Arc::new(AtomicBool::new(false)),
            }),
            diagnostics,
        ))
    }
}
