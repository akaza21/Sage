use super::*;

impl RuntimeSlot {
    pub(crate) fn active(&self) -> Option<Arc<RuntimeHost>> {
        match &*self.lock() {
            State::Primed { host, .. }
            | State::Claimed { host, .. }
            | State::Recovery { host, .. } => Some(Arc::clone(host)),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Stopping { .. }
            | State::Stopped => None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active().is_some()
    }

    pub(crate) fn owner(&self) -> Option<RuntimeOwner> {
        match &*self.lock() {
            State::Starting { owner }
            | State::StartingRecovery { owner }
            | State::Primed { owner, .. }
            | State::Claimed { owner, .. }
            | State::Recovery { owner, .. } => Some(owner.clone()),
            State::Stopping { owner, .. } => owner.clone(),
            State::Empty | State::Stopped => None,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(&*self.lock(), State::Empty)
    }
}
