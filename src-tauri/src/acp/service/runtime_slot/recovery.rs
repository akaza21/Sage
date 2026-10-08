use super::*;

impl RuntimeSlot {
    /// Clears the exact runtime recovery slot only after durable final-window
    /// recovery and projection publication have both succeeded.
    pub(crate) fn recover_after_zero_process(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<(), RuntimeSlotError> {
        let host = {
            let mut state = self.lock();
            match &*state {
                State::Recovery {
                    owner: current,
                    host,
                } if current == owner => Some(Arc::clone(host)),
                State::StartingRecovery { owner: current } if current == owner => {
                    self.config.clear(owner);
                    self.goal.clear(owner);
                    *state = State::Empty;
                    return Ok(());
                }
                State::Empty => return Ok(()),
                State::Stopping { .. } | State::Stopped => return Err(RuntimeSlotError::Stopping),
                State::Starting { .. }
                | State::StartingRecovery { .. }
                | State::Primed { .. }
                | State::Claimed { .. }
                | State::Recovery { .. } => return Err(RuntimeSlotError::Foreign),
            }
        };
        let host = host.expect("recovery host retained");
        host.shutdown(crate::acp::runtime::RuntimeHost::close_deadline())
            .map_err(|_| RuntimeSlotError::Recovery)?;
        let mut state = self.lock();
        match &*state {
            State::Recovery {
                owner: current,
                host: current_host,
            } if current == owner && Arc::ptr_eq(current_host, &host) => {
                self.config.clear(owner);
                self.goal.clear(owner);
                *state = State::Empty;
                Ok(())
            }
            _ => Err(RuntimeSlotError::Foreign),
        }
    }

    #[cfg(test)]
    pub(crate) fn recover_after_zero_process_fail_once_for_test(
        &self,
        owner: &RuntimeOwner,
        fail: &std::sync::atomic::AtomicBool,
    ) -> Result<(), RuntimeSlotError> {
        if fail.swap(false, std::sync::atomic::Ordering::AcqRel) {
            return Err(RuntimeSlotError::Recovery);
        }
        self.recover_after_zero_process(owner)
    }
}
