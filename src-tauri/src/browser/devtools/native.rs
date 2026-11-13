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
        CdpPort, highlight,
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
        developer: None,
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
        developer: None,
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

/// Clears the Agent's act marker, trying every step even after a failure,
/// and turns the overlay off again unless the tab annotates. Also releases
/// any developer-mode evaluation results still held (ADR-0009b), since this
/// runs when the Agent's control of the tab ends.
pub(crate) fn clear_agent_highlight(webview: &Webview) {
    let transport = NativeTransport {
        developer: None,
        webview: webview.clone(),
        permit: None,
        issued: Arc::new(AtomicBool::new(false)),
    };
    highlight::clear(&transport);
    super::cdp_port::release_developer_objects(&transport);
}

/// Removes the user's Annotate hover outline, outside any Agent permit.
pub(crate) fn clear_annotate_outline(webview: &Webview) -> Result<(), BrowserFailure> {
    let (method, params) = super::cdp_port::inspect::HIDE_OUTLINE;
    cdp::call(webview, method, params, 4096, Duration::from_secs(1))?;
    Ok(())
}

/// Turns the user's Annotate hover highlight on or off, outside any Agent
/// permit.
pub(crate) fn set_annotate(webview: &Webview, on: bool) -> Result<(), BrowserFailure> {
    let port = NativePort::new(NativeTransport {
        developer: None,
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
        developer: None,
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

pub(super) mod capture_scale;
mod dispatch;

pub(crate) type NativePort = CdpPort<NativeTransport>;
pub(crate) type NativeDevtools = DevtoolsCapability<NativePort>;

/// Carries Agent CDP commands to one WebView2 browser tab.
#[derive(Clone)]
pub(crate) struct NativeTransport {
    webview: Webview,
    developer: Option<Arc<super::developer_session::Session>>,
    permit: Option<BrowserDispatchPermit>,
    issued: Arc<AtomicBool>,
}

impl CdpTransport for NativeTransport {
    fn screenshot_scale(&self, timeout: Duration) -> Result<f64, BrowserFailure> {
        capture_scale::read(&self.webview, timeout)
    }

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
            cdp::Dispatch {
                issued: Some(self.issued.clone()),
                session: None,
            },
        )
    }

    fn raw(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        let session = self.developer.clone().ok_or(BrowserFailure::Denied)?;
        cdp::call_with_permit(
            &self.webview,
            method,
            params,
            max_result_bytes,
            timeout,
            self.permit.clone(),
            cdp::Dispatch {
                issued: Some(self.issued.clone()),
                session: Some(session),
            },
        )
    }

    // The evaluation and serializer share the group until the whole operation
    // finishes. Late completions and detach also clean up after timeouts.
    fn release_raw_objects(&self) {
        if let Some(session) = &self.developer {
            session.release_objects();
        }
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

    fn stopped(&self) -> bool {
        self.permit
            .as_ref()
            .is_some_and(|permit| !permit.is_live() && !permit.was_invalidated_by_navigation())
    }

    fn unless_annotating(&self, steps: &mut dyn FnMut()) {
        self.webview
            .app_handle()
            .state::<BrowserState>()
            .annotate
            .disable_unless_annotating(self.webview.label(), steps);
    }

    fn annotating(&self) -> bool {
        self.webview
            .app_handle()
            .state::<BrowserState>()
            .annotate
            .is_annotating(self.webview.label())
    }

    fn settled(&self, before: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
        if self.stopped() {
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
    pub(crate) fn for_attachment(
        state: &BrowserState,
        scope: BrowserScope,
        lease: &crate::acp::browser::attachment::AttachmentLease,
        generation: u64,
        permit: &BrowserDispatchPermit,
    ) -> Result<Self, BrowserFailure> {
        let capability = Self::for_tab_with_diagnostics(state, scope, false)?;
        let release = capability.release_buffers();
        lease
            .bind_resources(generation, || permit.is_live(), Box::new(release))
            .map_err(|_| BrowserFailure::Denied)?;
        Ok(capability)
    }

    pub(crate) fn wake_diagnostics(
        &mut self,
        lease: &crate::acp::browser::attachment::AttachmentLease,
        permit: &BrowserDispatchPermit,
    ) -> Result<(), BrowserFailure> {
        let previous = self.diagnostic_rings();
        let initial = {
            let previous = previous.lock().map_err(|_| BrowserFailure::Unavailable)?;
            if !previous.is_suspended() {
                return Ok(());
            }
            previous.resumed()
        };
        let scope = lease
            .diagnostic_scope()
            .ok_or(BrowserFailure::Unavailable)?;
        let webview = self.port_mut().transport.webview.clone();
        let rings = events::attach(&webview, Some(scope.clone()), Some(permit.clone()), initial)?;
        self.set_diagnostic_rings(rings.clone());
        // Setup registered its scoped cleanup before releasing the wake lock.
        Ok(())
    }

    #[cfg(feature = "native-lane")]
    pub(crate) fn for_tab(
        state: &BrowserState,
        scope: BrowserScope,
    ) -> Result<Self, BrowserFailure> {
        Self::for_tab_with_diagnostics(state, scope, true)
    }

    fn for_tab_with_diagnostics(
        state: &BrowserState,
        scope: BrowserScope,
        attach: bool,
    ) -> Result<Self, BrowserFailure> {
        let label = format!("browser-{}", scope.tab_id);
        let webview = state
            .tabs
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?
            .handle(&label)
            .cloned()
            .ok_or(BrowserFailure::Unavailable)?;
        Self::from_webview(scope, webview, attach)
    }

    fn from_webview(
        scope: BrowserScope,
        webview: Webview,
        attach: bool,
    ) -> Result<Self, BrowserFailure> {
        if webview.label() != format!("browser-{}", scope.tab_id) {
            return Err(BrowserFailure::Denied);
        }
        let diagnostics = if attach {
            events::attach(
                &webview,
                None,
                None,
                super::diagnostics::DiagnosticRings::default(),
            )?
        } else {
            let mut rings = super::diagnostics::DiagnosticRings::default();
            rings.suspend();
            Arc::new(std::sync::Mutex::new(rings))
        };
        Ok(Self::with_diagnostics(
            scope,
            NativePort::new(NativeTransport {
                developer: None,
                webview,
                permit: None,
                issued: Arc::new(AtomicBool::new(false)),
            }),
            diagnostics,
        ))
    }
}
