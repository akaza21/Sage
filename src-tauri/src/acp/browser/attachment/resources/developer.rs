//! A grant-scoped capability to use the tab-owned privileged native session.
//! Ordinary handles and diagnostics have independent lifetimes.
use super::*;

pub(crate) type DeveloperRelease = Arc<dyn Fn(bool) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct DeveloperScope {
    identity: Arc<AtomicBool>,
    resources: std::sync::Weak<Resources>,
    lane: u64,
    grant: u64,
    access: std::sync::Weak<super::super::developer::DeveloperAccess>,
}

impl DeveloperScope {
    pub(crate) fn with_live<T>(
        &self,
        label: &str,
        issue: impl FnOnce() -> Result<T, crate::acp::browser::BrowserFailure>,
    ) -> Result<T, crate::acp::browser::BrowserFailure> {
        self.with_access(|access| access.with_live(label, issue))
    }

    pub(crate) fn bind(
        &self,
        release: DeveloperRelease,
    ) -> Result<(), crate::acp::browser::BrowserFailure> {
        self.with_access(|access| access.bind(release))
    }

    fn with_access<T>(
        &self,
        issue: impl FnOnce(
            &super::super::developer::DeveloperAccess,
        ) -> Result<T, crate::acp::browser::BrowserFailure>,
    ) -> Result<T, crate::acp::browser::BrowserFailure> {
        use crate::acp::browser::BrowserFailure;
        let resources = self.resources.upgrade().ok_or(BrowserFailure::Denied)?;
        let _state = resources
            .state
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?;
        if !self.identity.load(Ordering::Acquire)
            || resources.generation.load(Ordering::Acquire) != self.lane
            || resources.grant_generation.load(Ordering::Acquire) != self.grant
        {
            return Err(BrowserFailure::Denied);
        }
        let access = self.access.upgrade().ok_or(BrowserFailure::Denied)?;
        issue(&access)
    }
}

impl AttachmentLease {
    pub(crate) fn developer_scope(&self) -> DeveloperScope {
        DeveloperScope {
            identity: self.active.clone(),
            resources: Arc::downgrade(&self.resources),
            lane: self.resource_generation(),
            grant: self.grant_generation(),
            access: Arc::downgrade(&self.developer),
        }
    }

    pub(in crate::acp::browser::attachment) fn release_developer_resources(&self) {
        self.developer.release_ungranted();
    }

    pub(in crate::acp::browser::attachment) fn sleep_developer_resources(&self, lane: Option<u64>) {
        if lane.is_none_or(|lane| lane == self.resource_generation()) {
            self.developer.sleep();
        }
    }
}
