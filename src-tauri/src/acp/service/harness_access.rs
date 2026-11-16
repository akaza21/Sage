//! The harness session mode a Turn the harness starts on its own may run
//! in: the one its chat's access choice maps to now. A chat's access lives
//! in the app's settings, so the app supplies the rule; until it does, no
//! such Turn opens.

use std::sync::{Arc, Mutex, PoisonError};

use sage_acp_protocol::scheduler::RuntimeOwner;

use super::AcpService;

pub(crate) type HarnessTurnAccess =
    Arc<dyn Fn(&RuntimeOwner) -> Option<&'static str> + Send + Sync>;

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
