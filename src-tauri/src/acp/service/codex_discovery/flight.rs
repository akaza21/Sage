use std::{
    sync::{Arc, Condvar, Mutex},
    time::Instant,
};

use crate::acp::{
    bindings::codex::CodexDiscoveryRunError,
    contract::{ProjectGeneration, SageProjectId, SageRootId},
    intent::BindingId,
    ipc::CodexDiscoveryResponse,
};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct DiscoveryFlightKey {
    pub(super) project_id: SageProjectId,
    pub(super) project_generation: ProjectGeneration,
    pub(super) root_id: SageRootId,
    pub(super) root_identity: [u8; 24],
    pub(super) binding_id: BindingId,
}

pub(super) struct DiscoveryFlight {
    key: DiscoveryFlightKey,
    result: Mutex<Option<Result<CodexDiscoveryResponse, CodexDiscoveryRunError>>>,
    changed: Condvar,
}

impl DiscoveryFlight {
    pub(super) fn new(key: DiscoveryFlightKey) -> Arc<Self> {
        Arc::new(Self {
            key,
            result: Mutex::new(None),
            changed: Condvar::new(),
        })
    }

    pub(super) fn matches_active(&self, key: &DiscoveryFlightKey) -> bool {
        self.key == *key && !self.is_complete()
    }

    pub(super) fn complete(&self, result: Result<CodexDiscoveryResponse, CodexDiscoveryRunError>) {
        *self
            .result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
        self.changed.notify_all();
    }

    fn is_complete(&self) -> bool {
        self.result
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    pub(super) fn wait_until(
        &self,
        deadline: Instant,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        let mut result = self
            .result
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        while result.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CodexDiscoveryRunError::Timeout);
            }
            let (next, wait) = self
                .changed
                .wait_timeout(result, remaining)
                .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
            result = next;
            if wait.timed_out() && result.is_none() {
                return Err(CodexDiscoveryRunError::Timeout);
            }
        }
        result
            .as_ref()
            .expect("completed discovery flight has a result")
            .clone()
    }
}
