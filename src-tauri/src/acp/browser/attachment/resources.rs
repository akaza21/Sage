//! Native lane authority and disposable diagnostics have separate lifetimes.
//! Sleeping drops listeners and history; revocation also cancels work and handles.
use super::*;
mod developer;
use developer::DeveloperRelease;
pub(crate) use developer::DeveloperScope;

#[derive(Clone)]
pub(crate) struct DiagnosticScope {
    identity: Arc<AtomicBool>,
    resources: std::sync::Weak<Resources>,
    lane: u64,
    wake: u64,
}

impl PartialEq for DiagnosticScope {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
            && self.lane == other.lane
            && self.wake == other.wake
    }
}
impl Eq for DiagnosticScope {}

impl DiagnosticScope {
    /// Registration and its epoch check share the resource lock, so delayed
    /// setup cannot replace listeners that have already woken for a new epoch.
    /// The unregister callback becomes revocable before setup releases its
    /// epoch lock, including while asynchronous domain enables complete.
    pub(crate) fn with_registered_diagnostics<T>(
        &self,
        install: impl FnOnce() -> Result<(T, Option<Release>), crate::acp::browser::BrowserFailure>,
    ) -> Result<T, crate::acp::browser::BrowserFailure> {
        use crate::acp::browser::BrowserFailure;
        let resources = self.resources.upgrade().ok_or(BrowserFailure::Denied)?;
        let mut state = resources
            .state
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?;
        if !self.identity.load(Ordering::Acquire)
            || resources.generation.load(Ordering::Acquire) != self.lane
            || state.wake != self.wake
        {
            return Err(BrowserFailure::Denied);
        }
        let (installed, release) = install()?;
        let previous = release.and_then(|release| state.diagnostics.replace((self.lane, release)));
        drop(state);
        if let Some((_, previous)) = previous {
            previous();
        }
        Ok(installed)
    }
}

type Release = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub(super) struct Resources {
    generation: std::sync::atomic::AtomicU64,
    grant_generation: std::sync::atomic::AtomicU64,
    state: Mutex<ResourceState>,
}

#[derive(Default)]
struct ResourceState {
    wake: u64,
    diagnostic_operations: usize,
    sleep_requested: Option<u64>,
    release: Option<(u64, Release)>,
    diagnostics: Option<(u64, Release)>,
    developer: Option<(u64, u64, DeveloperRelease)>,
}

impl AttachmentLease {
    pub(crate) fn grant_generation(&self) -> u64 {
        self.resources.grant_generation.load(Ordering::Acquire)
    }

    pub(super) fn invalidate_grant(&self) -> u64 {
        self.resources
            .grant_generation
            .fetch_add(1, Ordering::AcqRel)
    }

    pub(crate) fn begin_diagnostic_operation(
        &self,
    ) -> Result<DiagnosticOperation, crate::acp::browser::BrowserFailure> {
        let mut state = self
            .resources
            .state
            .lock()
            .map_err(|_| crate::acp::browser::BrowserFailure::Unavailable)?;
        state.diagnostic_operations = state
            .diagnostic_operations
            .checked_add(1)
            .ok_or(crate::acp::browser::BrowserFailure::Capacity)?;
        Ok(DiagnosticOperation {
            resources: self.resources.clone(),
        })
    }

    pub(crate) fn resource_generation(&self) -> u64 {
        self.resources.generation.load(Ordering::Acquire)
    }

    pub(crate) fn diagnostic_scope(&self) -> Option<DiagnosticScope> {
        let state = self.resources.state.lock().ok()?;
        Some(DiagnosticScope {
            identity: self.active.clone(),
            resources: Arc::downgrade(&self.resources),
            lane: self.resource_generation(),
            wake: state.wake,
        })
    }

    pub(crate) fn bind_resources(
        &self,
        generation: u64,
        current: impl FnOnce() -> bool,
        release: Release,
    ) -> Result<(), ()> {
        let Ok(mut state) = self.resources.state.lock() else {
            release();
            return Err(());
        };
        if self.resource_generation() != generation || !self.is_live() || !current() {
            drop(state);
            release();
            return Err(());
        }
        let previous = state.release.replace((generation, release));
        drop(state);
        if let Some((_, previous)) = previous {
            previous();
        }
        Ok(())
    }

    pub(super) fn release_resources(&self) {
        let generation = self.invalidate_resources();
        self.release_invalidated_resources(generation);
    }

    pub(super) fn invalidate_resources(&self) -> u64 {
        self.resources.generation.fetch_add(1, Ordering::AcqRel)
    }

    pub(super) fn release_invalidated_resources(&self, generation: u64) {
        self.release_developer_resources(Some(generation), None);
        let releases = self.resources.state.lock().ok().map(|mut state| {
            if state.sleep_requested == Some(generation) {
                state.sleep_requested = None;
            }
            let release = if state
                .release
                .as_ref()
                .is_some_and(|(owner, _)| *owner == generation)
            {
                state.release.take()
            } else {
                None
            };
            let diagnostics = if state
                .diagnostics
                .as_ref()
                .is_some_and(|(owner, _)| *owner == generation)
            {
                state.diagnostics.take()
            } else {
                None
            };
            (release, diagnostics)
        });
        if let Some((release, diagnostics)) = releases {
            if let Some((_, release)) = release {
                release();
            }
            if let Some((_, diagnostics)) = diagnostics {
                diagnostics();
            }
        }
    }

    pub(crate) fn sleep_diagnostics(&self, generation: Option<u64>) {
        self.sleep_developer_resources(generation);
        let release = self.resources.state.lock().ok().and_then(|mut state| {
            if generation.is_some_and(|generation| generation != self.resource_generation()) {
                return None;
            }
            if state.diagnostic_operations != 0 {
                state.sleep_requested = Some(self.resource_generation());
                return None;
            }
            state.sleep_requested = None;
            state.wake = state.wake.wrapping_add(1);
            state.diagnostics.take().map(|(_, release)| release)
        });
        if let Some(release) = release {
            release();
        }
    }
}

/// A pending operation may still be consuming diagnostics. Sleeping releases
/// them after its owned result is ready, without revoking ordinary authority.
pub(crate) struct DiagnosticOperation {
    resources: Arc<Resources>,
}

impl Drop for DiagnosticOperation {
    fn drop(&mut self) {
        let release = self.resources.state.lock().ok().and_then(|mut state| {
            state.diagnostic_operations -= 1;
            if state.diagnostic_operations == 0
                && state.sleep_requested == Some(self.resources.generation.load(Ordering::Acquire))
            {
                state.sleep_requested = None;
                state.wake = state.wake.wrapping_add(1);
                state.diagnostics.take().map(|(_, release)| release)
            } else {
                None
            }
        });
        if let Some(release) = release {
            release();
        }
    }
}

impl Drop for Resources {
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, release)) = state.release.take() {
            release();
        }
        if let Some((_, diagnostics)) = state.diagnostics.take() {
            diagnostics();
        }
        if let Some((_, _, release)) = state.developer.take() {
            release(true);
        }
    }
}

impl BrowserAttachments {
    pub(crate) fn suspend_tab(&self, label: &str) {
        if let Ok(entries) = self.entries.lock() {
            for lease in entries.values().filter(|lease| lease.tab_label == label) {
                lease.sleep_diagnostics(None);
            }
        }
        if let Ok(entries) = self.agent_entries.lock() {
            for lease in entries.values().filter(|lease| lease.tab_label == label) {
                lease.sleep_diagnostics(None);
            }
        }
    }
}
