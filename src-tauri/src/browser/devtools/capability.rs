use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crate::acp::browser::{
    BrowserAction, BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest,
    BrowserResult, BrowserScope, ElementHandle, SnapshotNode, ViewportRegion, WaitCondition,
    validate_request,
};

use super::{
    diagnostics::DiagnosticRings,
    snapshot::{DocumentStamp, NativeNode, project},
    wait::{WaitBudget, WaitStep},
};

pub(crate) trait DevtoolsPort {
    fn snapshot(&mut self) -> Result<(DocumentStamp, Vec<NativeNode>), BrowserFailure>;
    fn navigate(&mut self, _url: &str) -> Result<(String, u64), BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }
    fn act(
        &mut self,
        _stamp: DocumentStamp,
        _backend_id: u64,
        _action: &BrowserAction,
    ) -> Result<bool, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }
    fn capture(&mut self, region: Option<ViewportRegion>) -> Result<BrowserResult, BrowserFailure>;
    fn raw(&mut self, _method: &str, _params: &str) -> Result<String, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }
    /// Releases `DEVELOPER_OBJECT_GROUP` outside the dispatch permit, so it
    /// runs even when the permit was revoked mid-call.
    fn release_developer_objects(&mut self) {}
    fn matches(
        &mut self,
        condition: &WaitCondition,
        remaining: Duration,
    ) -> Result<bool, BrowserFailure>;
    fn stopped(&self) -> bool {
        false
    }
}

pub(crate) struct DevtoolsCapability<P> {
    scope: BrowserScope,
    port: P,
    diagnostics: Arc<Mutex<DiagnosticRings>>,
    cancelled: Arc<AtomicBool>,
    handles: Arc<Mutex<HashMap<String, (DocumentStamp, u64)>>>,
}

/// A dispatched mutation may have changed the page even when its result is
/// rejected, lost or timed out. Retire handles before any outcome escapes.
struct SnapshotInvalidation<'a>(&'a Mutex<HashMap<String, (DocumentStamp, u64)>>);

impl Drop for SnapshotInvalidation<'_> {
    fn drop(&mut self) {
        if let Ok(mut handles) = self.0.lock() {
            handles.clear();
        }
    }
}

impl<P: Clone> Clone for DevtoolsCapability<P> {
    fn clone(&self) -> Self {
        Self {
            scope: self.scope.clone(),
            port: self.port.clone(),
            diagnostics: self.diagnostics.clone(),
            cancelled: self.cancelled.clone(),
            handles: self.handles.clone(),
        }
    }
}

impl<P: DevtoolsPort> DevtoolsCapability<P> {
    #[cfg(test)]
    pub(super) fn new(scope: BrowserScope, port: P) -> Self {
        Self::with_diagnostics(
            scope,
            port,
            Arc::new(Mutex::new(DiagnosticRings::default())),
        )
    }

    pub(super) fn with_diagnostics(
        scope: BrowserScope,
        port: P,
        diagnostics: Arc<Mutex<DiagnosticRings>>,
    ) -> Self {
        Self {
            scope,
            port,
            diagnostics,
            cancelled: Arc::new(AtomicBool::new(false)),
            handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    pub(super) fn diagnostics(&self) -> Arc<Mutex<DiagnosticRings>> {
        self.diagnostics.clone()
    }

    pub(super) fn port_mut(&mut self) -> &mut P {
        &mut self.port
    }

    /// Small shared state may outlive the cache entry in an in-flight call;
    /// release its allocations and stop waits without retaining the lane.
    pub(super) fn release_buffers(&self) -> impl FnOnce() + Send + 'static {
        let (cancelled, handles) = (self.cancelled.clone(), self.handles.clone());
        move || {
            cancelled.store(true, Ordering::SeqCst);
            if let Ok(mut handles) = handles.lock() {
                *handles = HashMap::new();
            }
        }
    }

    #[cfg(windows)]
    pub(super) fn diagnostic_rings(&self) -> Arc<Mutex<DiagnosticRings>> {
        self.diagnostics.clone()
    }

    #[cfg(windows)]
    pub(super) fn set_diagnostic_rings(&mut self, rings: Arc<Mutex<DiagnosticRings>>) {
        self.diagnostics = rings;
    }

    #[cfg(test)]
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    fn dispatch_mutation<R>(
        &mut self,
        dispatch: impl FnOnce(&mut P) -> Result<R, BrowserFailure>,
    ) -> Result<R, BrowserFailure> {
        // Do not hold the handle lock while the page runs. The guard also
        // covers future early returns inside the dispatch closure.
        let _invalidate = SnapshotInvalidation(&self.handles);
        dispatch(&mut self.port)
    }

    fn wait_for(
        &mut self,
        condition: &WaitCondition,
        timeout_ms: u32,
    ) -> Result<BrowserResult, BrowserFailure> {
        let budget = WaitBudget::new(timeout_ms)?;
        let started = Instant::now();
        loop {
            let elapsed = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
            match budget.step(elapsed, self.cancelled.load(Ordering::SeqCst)) {
                WaitStep::Cancelled => return Err(BrowserFailure::Denied),
                WaitStep::TimedOut => return Ok(BrowserResult::WaitFor { matched: false }),
                WaitStep::Probe => {}
            }
            if self.port.stopped() {
                return Err(BrowserFailure::Denied);
            }
            let remaining = timeout_ms.saturating_sub(elapsed);
            if self.port.matches(
                condition,
                Duration::from_millis(u64::from(remaining.min(5_000))),
            )? {
                return Ok(BrowserResult::WaitFor { matched: true });
            }
            let elapsed = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
            let remaining = timeout_ms.saturating_sub(elapsed);
            if remaining > 0 {
                thread::sleep(Duration::from_millis(u64::from(remaining.min(100))));
            }
        }
    }
}

impl<P: DevtoolsPort> BrowserCapability for DevtoolsCapability<P> {
    fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        validate_request(request)?;
        if request.scope != self.scope || self.cancelled.load(Ordering::SeqCst) {
            return Err(BrowserFailure::Denied);
        }
        match &request.command {
            BrowserCommand::OpenTab { .. } => Err(BrowserFailure::InvalidRequest),
            BrowserCommand::Navigate { url } => {
                let (url, navigation_epoch) = self.dispatch_mutation(|port| port.navigate(url))?;
                Ok(BrowserResult::Navigate {
                    url,
                    navigation_epoch,
                })
            }
            BrowserCommand::SnapshotPage {} => {
                let (stamp, native_nodes) = self.port.snapshot()?;
                let mut handles = self
                    .handles
                    .lock()
                    .map_err(|_| BrowserFailure::Unavailable)?;
                if self.cancelled.load(Ordering::SeqCst) {
                    return Err(BrowserFailure::Denied);
                }
                handles.clear();
                let (text, nodes) = project(native_nodes)?;
                let nodes = nodes
                    .into_iter()
                    .map(|node| {
                        let token = uuid::Uuid::new_v4().simple().to_string();
                        handles.insert(token.clone(), (stamp, node.backend_id));
                        SnapshotNode {
                            handle: ElementHandle::from_token(token),
                            role: node.role,
                            name: node.name,
                        }
                    })
                    .collect();
                Ok(BrowserResult::SnapshotPage {
                    navigation_epoch: stamp.navigation_id,
                    text,
                    nodes,
                })
            }
            BrowserCommand::Screenshot { region } => self.port.capture(*region),
            BrowserCommand::ReadConsole { cursor, limit } => {
                let (next_cursor, entries) = self
                    .diagnostics
                    .lock()
                    .map_err(|_| BrowserFailure::Unavailable)?
                    .console(*cursor, *limit)?;
                Ok(BrowserResult::ReadConsole {
                    next_cursor,
                    entries,
                })
            }
            BrowserCommand::ReadNetwork { cursor, limit } => {
                let (next_cursor, entries) = self
                    .diagnostics
                    .lock()
                    .map_err(|_| BrowserFailure::Unavailable)?
                    .network(*cursor, *limit)?;
                Ok(BrowserResult::ReadNetwork {
                    next_cursor,
                    entries,
                })
            }
            BrowserCommand::Act { action } => {
                let (stamp, backend_id) = self
                    .handles
                    .lock()
                    .map_err(|_| BrowserFailure::Unavailable)?
                    .get(action.handle().token())
                    .copied()
                    .ok_or(BrowserFailure::StaleHandle)?;
                let applied = self.dispatch_mutation(|port| port.act(stamp, backend_id, action))?;
                Ok(BrowserResult::Act { applied })
            }
            BrowserCommand::WaitFor {
                condition,
                timeout_ms,
            } => self.wait_for(condition, *timeout_ms),
            BrowserCommand::Cdp { method, params } => {
                let evaluate = method == "Runtime.evaluate";
                let mut params = params.clone();
                if evaluate {
                    // Objects stay remote until CdpPort serializes within its
                    // byte budget. Release our group on every outcome.
                    params.insert("returnByValue".into(), false.into());
                    params.insert(
                        "objectGroup".into(),
                        crate::acp::browser::developer::DEVELOPER_OBJECT_GROUP.into(),
                    );
                    let limit = crate::acp::browser::developer::EVALUATE_TIMEOUT_MS;
                    let timeout = params
                        .get("timeout")
                        .and_then(serde_json::Value::as_u64)
                        .map_or(limit, |asked| asked.min(limit));
                    params.insert("timeout".into(), timeout.into());
                }
                let params =
                    serde_json::to_string(&params).map_err(|_| BrowserFailure::InvalidRequest)?;
                let result = self.dispatch_mutation(|port| {
                    let result = port.raw(method, &params);
                    if evaluate {
                        port.release_developer_objects();
                    }
                    result
                })?;
                Ok(BrowserResult::Cdp {
                    result: serde_json::from_str(&result)
                        .map_err(|_| BrowserFailure::Unavailable)?,
                })
            }
        }
    }
}
