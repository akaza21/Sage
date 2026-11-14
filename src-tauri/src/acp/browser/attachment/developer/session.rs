use super::*;
use crate::acp::browser::BrowserFailure;
use resources::DeveloperRelease;

impl TabGrants {
    pub(super) fn release_ungranted(&mut self) {
        if !self.grants.values().any(DeveloperGrant::is_live) {
            // A later approval must never revive a command revoked during
            // handoff, even while its old attachment is still draining.
            for grant in self.grants.values_mut().filter(|grant| grant.draining) {
                grant.revoked = true;
            }
            if let Some(release) = self.release.take() {
                // The native hook only closes session state. Keep the grant lock
                // until it returns so re-approval cannot acquire a closing session.
                release(true);
            }
        }
    }
}

impl DeveloperAccess {
    pub(in crate::acp::browser::attachment) fn with_live<T>(
        &self,
        label: &str,
        issue: impl FnOnce() -> Result<T, BrowserFailure>,
    ) -> Result<T, BrowserFailure> {
        let tab = self.tab.lock().map_err(|_| BrowserFailure::Unavailable)?;
        if label != self.label
            || !tab
                .grants
                .get(&self.key())
                .is_some_and(DeveloperGrant::is_live)
        {
            return Err(BrowserFailure::Denied);
        }
        issue()
    }

    pub(in crate::acp::browser::attachment) fn bind(
        &self,
        release: DeveloperRelease,
    ) -> Result<(), BrowserFailure> {
        let mut tab = self.tab.lock().map_err(|_| BrowserFailure::Unavailable)?;
        if !tab
            .grants
            .get(&self.key())
            .is_some_and(DeveloperGrant::is_live)
        {
            return Err(BrowserFailure::Denied);
        }
        let previous = tab.release.replace(release);
        drop(tab);
        if let Some(previous) = previous {
            previous(true);
        }
        Ok(())
    }

    pub(in crate::acp::browser::attachment) fn release_ungranted(&self) {
        if let Ok(mut tab) = self.tab.lock() {
            tab.release_ungranted();
        }
    }

    pub(in crate::acp::browser::attachment) fn sleep(&self) {
        if let Ok(tab) = self.tab.lock()
            && let Some(release) = &tab.release
        {
            release(false);
        }
    }
}

impl Drop for DeveloperAccess {
    fn drop(&mut self) {
        let grant = self
            .tab
            .lock()
            .ok()
            .and_then(|mut tab| tab.grants.remove(&self.key()));
        self.release_ungranted();
        drop(grant);
    }
}
