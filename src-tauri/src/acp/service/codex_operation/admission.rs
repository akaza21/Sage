use std::sync::{Arc, Mutex};

use super::OperationClass;

/// Admission is bounded by the live lane count across both classes. At most
/// one ephemeral operation is admitted at a time, and it may run beside live
/// sessions whenever capacity is spare.
#[derive(Default)]
pub(super) struct AdmissionState {
    pub(super) ephemeral: bool,
    pub(super) live: usize,
}

impl AdmissionState {
    pub(super) fn occupied(&self) -> usize {
        self.live + usize::from(self.ephemeral)
    }
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
