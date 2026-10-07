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
    fn matches(
        &mut self,
        condition: &WaitCondition,
        remaining: Duration,
    ) -> Result<bool, BrowserFailure>;
}

pub(crate) struct DevtoolsCapability<P> {
    scope: BrowserScope,
    port: P,
    diagnostics: Arc<Mutex<DiagnosticRings>>,
    cancelled: Arc<AtomicBool>,
    handles: Arc<Mutex<HashMap<String, (DocumentStamp, u64)>>>,
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

    #[cfg(test)]
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
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
                let (url, navigation_epoch) = self.port.navigate(url)?;
                self.handles
                    .lock()
                    .map_err(|_| BrowserFailure::Unavailable)?
                    .clear();
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
                let applied = self.port.act(stamp, backend_id, action)?;
                Ok(BrowserResult::Act { applied })
            }
            BrowserCommand::WaitFor {
                condition,
                timeout_ms,
            } => self.wait_for(condition, *timeout_ms),
        }
    }
}
