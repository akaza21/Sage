use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::watch;

use super::super::codex_operation::CodexOperationCancellation;
use crate::acp::bindings::codex::CodexDiscoveryRunError;

#[derive(Default)]
struct Cancellation {
    requested: bool,
    control: Option<Arc<dyn CodexOperationCancellation>>,
}

/// One observation may own the ephemeral slot. Foreground commands cancel it
/// and await zero-process settlement before attempting their own admission.
pub(super) struct Observation {
    cancellation: Mutex<Cancellation>,
    finished: watch::Sender<Option<Result<(), CodexDiscoveryRunError>>>,
}

impl Observation {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            cancellation: Mutex::new(Cancellation::default()),
            finished: watch::channel(None).0,
        })
    }

    pub(super) fn register(
        &self,
        control: Arc<dyn CodexOperationCancellation>,
    ) -> Result<(), CodexDiscoveryRunError> {
        let mut cancellation = self
            .cancellation
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        if cancellation.requested {
            return Err(CodexDiscoveryRunError::Drift);
        }
        cancellation.control = Some(control);
        Ok(())
    }

    pub(super) fn ensure_current(&self) -> Result<(), CodexDiscoveryRunError> {
        if self
            .cancellation
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?
            .requested
        {
            Err(CodexDiscoveryRunError::Drift)
        } else {
            Ok(())
        }
    }

    pub(super) async fn yield_until(
        &self,
        deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError> {
        let mut finished = self.finished.subscribe();
        if let Some(result) = *finished.borrow() {
            return result;
        }
        let control = {
            let mut cancellation = self
                .cancellation
                .lock()
                .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
            cancellation.requested = true;
            cancellation.control.clone()
        };
        if let Some(control) = control
            && let Err(error) = control.request_parent_shutdown_until(deadline)
        {
            return finished.borrow().unwrap_or(Err(error));
        }
        let result = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            finished.wait_for(Option::is_some),
        )
        .await
        .map_err(|_| CodexDiscoveryRunError::Timeout)?
        .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        result.unwrap_or(Err(CodexDiscoveryRunError::Cleanup))
    }
}

pub(super) struct ObservationLease {
    pub(super) observation: Arc<Observation>,
    pub(super) settlement: Result<(), CodexDiscoveryRunError>,
}

impl Drop for ObservationLease {
    fn drop(&mut self) {
        self.observation
            .finished
            .send_replace(Some(self.settlement));
    }
}
