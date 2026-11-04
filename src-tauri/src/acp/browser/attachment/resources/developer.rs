//! Revocable ownership of the privileged native session, independently of
//! ordinary handles and diagnostics. `force=false` lets owned work finish.
use super::*;

pub(crate) type DeveloperRelease = Arc<dyn Fn(bool) + Send + Sync>;

#[derive(Clone)]
pub(crate) struct DeveloperScope {
    identity: Arc<AtomicBool>,
    resources: std::sync::Weak<Resources>,
    lane: u64,
    grant: u64,
}

impl PartialEq for DeveloperScope {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.identity, &other.identity)
            && self.lane == other.lane
            && self.grant == other.grant
    }
}
impl Eq for DeveloperScope {}

impl DeveloperScope {
    pub(crate) fn bind(
        &self,
        release: DeveloperRelease,
    ) -> Result<(), crate::acp::browser::BrowserFailure> {
        use crate::acp::browser::BrowserFailure;
        let resources = self.resources.upgrade().ok_or(BrowserFailure::Denied)?;
        let mut state = resources
            .state
            .lock()
            .map_err(|_| BrowserFailure::Unavailable)?;
        if !self.identity.load(Ordering::Acquire)
            || resources.generation.load(Ordering::Acquire) != self.lane
            || resources.grant_generation.load(Ordering::Acquire) != self.grant
        {
            return Err(BrowserFailure::Denied);
        }
        let previous = state.developer.replace((self.lane, self.grant, release));
        drop(state);
        if let Some((_, _, release)) = previous {
            release(true);
        }
        Ok(())
    }
}

impl AttachmentLease {
    pub(crate) fn developer_scope(&self) -> DeveloperScope {
        DeveloperScope {
            identity: self.active.clone(),
            resources: Arc::downgrade(&self.resources),
            lane: self.resource_generation(),
            grant: self.grant_generation(),
        }
    }

    pub(in crate::acp::browser::attachment) fn release_developer_resources(
        &self,
        lane: Option<u64>,
        grant: Option<u64>,
    ) {
        let release = self.resources.state.lock().ok().and_then(|mut state| {
            if state.developer.as_ref().is_some_and(|(l, g, _)| {
                lane.is_none_or(|lane| lane == *l) && grant.is_none_or(|grant| grant == *g)
            }) {
                state.developer.take()
            } else {
                None
            }
        });
        if let Some((_, _, release)) = release {
            release(true);
        }
    }

    pub(in crate::acp::browser::attachment) fn sleep_developer_resources(&self, lane: Option<u64>) {
        let release = self.resources.state.lock().ok().and_then(|state| {
            state
                .developer
                .as_ref()
                .filter(|(l, _, _)| lane.is_none_or(|lane| lane == *l))
                .map(|(_, _, release)| release.clone())
        });
        if let Some(release) = release {
            release(false);
        }
    }
}
