//! The access a Turn the harness starts on its own may run under: its
//! chat's access choice now, with the harness session mode that maps to
//! when the binding has one. A chat's access lives in the app's settings,
//! so the app supplies the rule; until it does, no such Turn opens.

use std::sync::{Arc, Mutex, PoisonError};

use sage_acp_protocol::{access_mode::SessionAccessMode, scheduler::RuntimeOwner};

use super::AcpService;

pub(crate) type HarnessTurnAccess =
    Arc<dyn Fn(&RuntimeOwner) -> Option<(SessionAccessMode, Option<&'static str>)> + Send + Sync>;

#[derive(Default)]
pub(crate) struct HarnessAccessSlot(Mutex<Option<HarnessTurnAccess>>);

impl HarnessAccessSlot {
    pub(super) fn get(&self) -> Option<HarnessTurnAccess> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn set(&self, access: HarnessTurnAccess) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(access);
    }
}

impl AcpService {
    pub(crate) fn set_harness_turn_access(&self, access: HarnessTurnAccess) {
        for lane in self.lanes() {
            lane.runtime.harness_access.set(Arc::clone(&access));
        }
    }
}
