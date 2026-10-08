use super::*;

impl RuntimeSlot {
    pub(crate) fn begin_shutdown(&self) -> Option<Arc<RuntimeHost>> {
        let mut state = self.lock();
        self.config.clear_all();
        self.goal.clear_all();
        let retained = match &*state {
            State::Primed { owner, host }
            | State::Claimed { owner, host }
            | State::Recovery { owner, host } => Some((owner.clone(), Some(Arc::clone(host)))),
            State::Starting { owner } | State::StartingRecovery { owner } => {
                Some((owner.clone(), None))
            }
            State::Empty | State::Stopping { .. } | State::Stopped => None,
        };
        if matches!(&*state, State::Empty) {
            *state = State::Stopping {
                owner: None,
                host: None,
            };
        } else if let Some((owner, host)) = &retained {
            *state = State::Stopping {
                owner: Some(owner.clone()),
                host: host.as_ref().map(Arc::clone),
            };
        }
        retained.and_then(|(_, host)| host)
    }

    pub(crate) fn shutdown_failed(&self) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        let State::Stopping {
            owner: Some(owner),
            host: Some(host),
        } = &*state
        else {
            return Err(RuntimeSlotError::Stopping);
        };
        *state = State::Recovery {
            owner: owner.clone(),
            host: Arc::clone(host),
        };
        Ok(())
    }

    pub(crate) fn finish_shutdown(&self) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        if !matches!(&*state, State::Stopping { .. }) {
            return Err(RuntimeSlotError::Recovery);
        }
        *state = State::Stopped;
        Ok(())
    }
}
