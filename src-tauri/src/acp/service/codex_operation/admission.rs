use std::sync::{Arc, Mutex};

use super::OperationClass;

/// Admission is exclusive for ephemeral work and shared, up to the live lane
/// count, for live sessions. Ephemeral and live work never overlap.
#[derive(Default)]
pub(super) struct AdmissionState {
    pub(super) ephemeral: bool,
    pub(super) live: usize,
}

pub(super) struct AdmissionLease {
    pub(super) admission: Arc<Mutex<AdmissionState>>,
    pub(super) class: OperationClass,
}

impl Drop for AdmissionLease {
    fn drop(&mut self) {
        let mut admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.class {
            OperationClass::Ephemeral => admission.ephemeral = false,
            OperationClass::Live => admission.live = admission.live.saturating_sub(1),
        }
    }
}
