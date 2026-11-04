//! Issue-time and completion-time authority checks for native dispatch.
use super::*;

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
            || self
                .elevated_is_current
                .as_ref()
                .is_some_and(|check| !check())
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
                && self
                    .elevated_is_current
                    .as_ref()
                    .is_none_or(|check| check())
        })
    }

    pub(super) fn owner_current(&self) -> bool {
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
            && self.elevated_is_current.as_ref().is_none_or(|check| check())
    }

    pub fn revoke(&self) {
        self.authority.stop();
    }
}
