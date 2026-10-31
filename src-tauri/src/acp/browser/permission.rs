//! Browser-specific permission gate. A UI-backed decider is required before admission.
use super::{
    BrowserCapability, BrowserCommand, BrowserFailure, BrowserRequest, BrowserResult, BrowserScope,
    developer::DeveloperModePolicy,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserPermissionChoice {
    AllowOnce,
    AllowForOriginSession,
    /// Developer mode for this chat session, tab and origin (ADR-0009b).
    /// Only an elevated prompt admits it, and it admits nothing else.
    AllowDeveloperMode,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserPermissionPrompt {
    pub scope: BrowserScope,
    pub origin: String,
    pub operation: &'static str,
}

pub trait BrowserPermissionDecider {
    fn decide(&mut self, prompt: &BrowserPermissionPrompt) -> BrowserPermissionChoice;
}

pub type PendingBrowserResult = Box<dyn FnOnce() -> Result<BrowserResult, BrowserFailure> + Send>;

pub trait BrowserCapabilityDispatch {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure>;
}

impl<T: BrowserCapabilityDispatch + ?Sized> BrowserCapabilityDispatch for &mut T {
    fn dispatch(
        &mut self,
        request: &BrowserRequest,
        permit: BrowserDispatchPermit,
    ) -> Result<PendingBrowserResult, BrowserFailure> {
        (**self).dispatch(request, permit)
    }
}

#[derive(Clone)]
pub struct BrowserDispatchPermit {
    authority: BrowserAuthority,
    scope: BrowserScope,
    revision: u64,
    owner_generation: u64,
    tab_is_current: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    owner_is_current: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    owner_witness: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    issued: Arc<AtomicBool>,
}

impl BrowserDispatchPermit {
    pub fn with_live_lock<T>(
        &self,
        issue: impl FnOnce() -> Result<T, BrowserFailure>,
    ) -> Result<T, BrowserFailure> {
        if !self.owner_current() {
            return Err(BrowserFailure::Denied);
        }
        let state = self
            .authority
            .0
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?;
        if !state.active
            || state.scope != self.scope
            || state.revision != self.revision
            || state.owner_generation != self.owner_generation
            || self.tab_is_current.as_ref().is_some_and(|check| !check())
            || self.owner_witness.as_ref().is_some_and(|check| !check())
        {
            return Err(BrowserFailure::Denied);
        }
        issue()
    }

    pub(crate) fn is_live(&self) -> bool {
        self.authority.0.lock().is_ok_and(|state| {
            state.active
                && state.scope == self.scope
                && state.revision == self.revision
                && state.owner_generation == self.owner_generation
                && self.tab_is_current.as_ref().is_none_or(|check| check())
                && self.owner_witness.as_ref().is_none_or(|check| check())
        })
    }

    fn owner_current(&self) -> bool {
        self.owner_is_current.as_ref().is_none_or(|check| check())
    }

    pub(crate) fn mark_issued(&self) {
        self.issued.store(true, Ordering::Release);
    }

    pub(crate) fn revoked_failure(&self) -> BrowserFailure {
        if self.issued.load(Ordering::Acquire) {
            BrowserFailure::OutcomeUnknown
        } else {
            BrowserFailure::Denied
        }
    }

    pub(crate) fn was_invalidated_by_navigation(&self) -> bool {
        self.authority.was_invalidated_by_navigation(self.revision)
    }

    pub fn revoke(&self) {
        self.authority.stop();
    }
}

impl<F: FnMut(&BrowserPermissionPrompt) -> BrowserPermissionChoice> BrowserPermissionDecider for F {
    fn decide(&mut self, prompt: &BrowserPermissionPrompt) -> BrowserPermissionChoice {
        self(prompt)
    }
}

#[derive(Clone)]
pub struct BrowserAuthority(Arc<Mutex<AuthorityState>>);

struct AuthorityState {
    scope: BrowserScope,
    origin: String,
    owner_generation: u64,
    revision: u64,
    active: bool,
    grant_origin: Option<String>,
    navigation_invalidation: bool,
}

impl BrowserAuthority {
    pub fn attached(
        scope: BrowserScope,
        origin: &str,
        owner_generation: u64,
    ) -> Result<Self, BrowserFailure> {
        Ok(Self(Arc::new(Mutex::new(AuthorityState {
            scope,
            origin: normalized_origin(origin)?,
            owner_generation,
            revision: 1,
            active: true,
            grant_origin: None,
            navigation_invalidation: false,
        }))))
    }

    pub fn set_origin(&self, origin: &str) -> Result<(), BrowserFailure> {
        let origin = normalized_origin(origin)?;
        let mut state = self.0.lock().map_err(|_| BrowserFailure::Unavailable)?;
        state.origin = origin;
        state.revision = state.revision.saturating_add(1);
        state.grant_origin = None;
        state.navigation_invalidation = false;
        Ok(())
    }

    pub fn owner_changed(&self, generation: u64) {
        if let Ok(mut state) = self.0.lock() {
            state.owner_generation = generation;
            state.revision = state.revision.saturating_add(1);
            state.grant_origin = None;
            state.navigation_invalidation = false;
        }
    }

    pub fn stop(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.active = false;
            state.revision = state.revision.saturating_add(1);
            state.grant_origin = None;
            state.navigation_invalidation = false;
        }
    }

    pub fn navigation_started(&self) {
        if let Ok(mut state) = self.0.lock() {
            state.active = false;
            state.revision = state.revision.saturating_add(1);
            state.grant_origin = None;
            state.navigation_invalidation = true;
        }
    }

    fn was_invalidated_by_navigation(&self, revision: u64) -> bool {
        self.0
            .lock()
            .is_ok_and(|state| state.navigation_invalidation && state.revision > revision)
    }

    pub fn close_tab(&self) {
        self.stop();
    }

    pub(crate) fn liveness_for(
        &self,
        prompt: &BrowserPermissionPrompt,
    ) -> Option<impl Fn() -> bool + Send + Sync + 'static> {
        let state = self.0.lock().ok()?;
        if !state.active || state.scope != prompt.scope {
            return None;
        }
        let revision = state.revision;
        let scope = prompt.scope.clone();
        let authority = self.clone();
        Some(move || {
            authority.0.lock().is_ok_and(|state| {
                state.active && state.scope == scope && state.revision == revision
            })
        })
    }
}

fn normalized_origin(value: &str) -> Result<String, BrowserFailure> {
    let url = url::Url::parse(value).map_err(|_| BrowserFailure::InvalidRequest)?;
    if !matches!(url.scheme(), "http" | "https") || url.username() != "" || url.password().is_some()
    {
        return Err(BrowserFailure::InvalidRequest);
    }
    Ok(url.origin().ascii_serialization())
}

pub struct BrowserPermissionGate<P, D> {
    port: P,
    decider: D,
    authority: BrowserAuthority,
    tab_is_current: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    owner_is_current: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    owner_witness: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    developer: Option<Arc<dyn DeveloperModePolicy>>,
}

impl<P, D> BrowserPermissionGate<P, D> {
    pub fn new(port: P, decider: D, authority: BrowserAuthority) -> Self {
        Self {
            port,
            decider,
            authority,
            tab_is_current: None,
            owner_is_current: None,
            owner_witness: None,
            developer: None,
        }
    }

    /// Admits elevated commands under `policy`; without it they are denied.
    pub fn with_developer_mode(mut self, policy: Arc<dyn DeveloperModePolicy>) -> Self {
        self.developer = Some(policy);
        self
    }

    pub fn with_tab_revision_guard(
        mut self,
        check: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        self.tab_is_current = Some(Arc::new(check));
        self
    }

    pub fn with_owner_guard(mut self, check: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.owner_is_current = Some(Arc::new(check));
        self
    }

    pub fn with_owner_witness(mut self, check: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.owner_witness = Some(Arc::new(check));
        self
    }
}

impl<P: BrowserCapabilityDispatch, D: BrowserPermissionDecider> BrowserCapability
    for BrowserPermissionGate<P, D>
{
    fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure> {
        let BrowserCommand::Cdp { method, params } = &request.command else {
            return self.perform_admitted(request, None);
        };
        self.perform_elevated(request, method, params)
    }
}

impl<P: BrowserCapabilityDispatch, D: BrowserPermissionDecider> BrowserPermissionGate<P, D> {
    /// Asks, unless a grant already covers the request, then dispatches under
    /// a permit. `elevated` carries the developer policy and the origin the
    /// command was admitted for; an ordinary grant never covers it, and a
    /// developer grant covers nothing else.
    fn perform_admitted(
        &mut self,
        request: &BrowserRequest,
        elevated: Option<(&dyn DeveloperModePolicy, &str, &dyn Fn() -> bool)>,
    ) -> Result<BrowserResult, BrowserFailure> {
        let session = request.scope.session_id.as_str();
        let (revision, owner_generation, origin, already_granted) = {
            let state = self
                .authority
                .0
                .lock()
                .map_err(|_| BrowserFailure::Unavailable)?;
            if !state.active || state.scope != request.scope {
                return Err(BrowserFailure::Denied);
            }
            let origin = match &request.command {
                BrowserCommand::OpenTab { url } | BrowserCommand::Navigate { url } => {
                    normalized_origin(url)?
                }
                _ => state.origin.clone(),
            };
            let granted = match elevated {
                Some((_, admitted, _)) if admitted != origin => return Err(BrowserFailure::Denied),
                Some((developer, _, _)) => developer.granted(session, &origin),
                None => state.grant_origin.as_deref() == Some(origin.as_str()),
            };
            (state.revision, state.owner_generation, origin, granted)
        };
        let choice = match (already_granted, elevated.is_some()) {
            (true, true) => BrowserPermissionChoice::AllowDeveloperMode,
            (true, false) => BrowserPermissionChoice::AllowForOriginSession,
            (false, _) => self.decider.decide(&BrowserPermissionPrompt {
                scope: request.scope.clone(),
                origin: origin.clone(),
                operation: operation(&request.command),
            }),
        };
        let admitted = match choice {
            BrowserPermissionChoice::AllowDeveloperMode => elevated.is_some(),
            BrowserPermissionChoice::AllowOnce | BrowserPermissionChoice::AllowForOriginSession => {
                elevated.is_none()
            }
            BrowserPermissionChoice::Deny => false,
        };
        if !admitted {
            return Err(BrowserFailure::Denied);
        }
        // The attachment holds the grant, so Stop, Take over and a
        // cross-origin commit end it whichever authority asked. The switch
        // is read again because it may have gone off while the card was
        // open, and the command leaves Sage only once its audit row is
        // written.
        if let Some((developer, _, record_sent)) = elevated
            && (!developer.enabled()
                || (!already_granted && !developer.grant(session, &origin))
                || !record_sent())
        {
            return Err(BrowserFailure::Denied);
        }
        let permit = BrowserDispatchPermit {
            authority: self.authority.clone(),
            scope: request.scope.clone(),
            revision,
            owner_generation,
            tab_is_current: self.tab_is_current.clone(),
            owner_is_current: self.owner_is_current.clone(),
            owner_witness: self.owner_witness.clone(),
            issued: Arc::new(AtomicBool::new(false)),
        };
        let pending = self.port.dispatch(request, permit.clone())?;
        // The native port issues under permit.with_live_lock in its UI callback;
        // completion waits outside that lock so Stop/navigation can revoke it.
        let outcome = pending();
        if matches!(&outcome, Err(BrowserFailure::OutcomeUnknown)) {
            return Err(BrowserFailure::OutcomeUnknown);
        }
        if !permit.owner_current() {
            return Err(permit.revoked_failure());
        }
        let navigation_result = matches!(
            (&request.command, &outcome),
            (
                BrowserCommand::Navigate { .. },
                Ok(BrowserResult::Navigate { .. })
            ) | (
                BrowserCommand::Act { .. },
                Ok(BrowserResult::Act { applied: true })
            )
        ) && self.authority.was_invalidated_by_navigation(revision);
        if !permit.is_live() && !navigation_result {
            return Err(permit.revoked_failure());
        }
        let result = outcome?;
        if navigation_result {
            return Ok(result);
        }
        let mut state = self
            .authority
            .0
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?;
        if !state.active
            || state.scope != request.scope
            || state.revision != revision
            || state.owner_generation != owner_generation
        {
            return Err(permit.revoked_failure());
        }
        if choice == BrowserPermissionChoice::AllowForOriginSession {
            state.grant_origin = Some(origin.clone());
        }
        if let (BrowserCommand::Navigate { .. }, BrowserResult::Navigate { url, .. }) =
            (&request.command, &result)
        {
            let committed = normalized_origin(url)?;
            if state.origin != committed {
                state.origin = committed.clone();
                state.revision = state.revision.saturating_add(1);
                if state.grant_origin.as_deref() != Some(committed.as_str()) {
                    state.grant_origin = None;
                }
            }
        }
        Ok(result)
    }
}

fn operation(command: &BrowserCommand) -> &'static str {
    match command {
        BrowserCommand::OpenTab { .. } => "openTab",
        BrowserCommand::Navigate { .. } => "navigate",
        BrowserCommand::SnapshotPage {} => "snapshotPage",
        BrowserCommand::Act { .. } => "act",
        BrowserCommand::WaitFor { .. } => "waitFor",
        BrowserCommand::Screenshot { .. } => "screenshot",
        BrowserCommand::ReadConsole { .. } => "readConsole",
        BrowserCommand::ReadNetwork { .. } => "readNetwork",
        BrowserCommand::Cdp { .. } => "cdp",
    }
}

#[path = "permission/developer.rs"]
mod developer;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "permission/developer_tests.rs"]
mod developer_tests;
