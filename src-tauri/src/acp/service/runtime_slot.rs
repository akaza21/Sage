use std::sync::{Arc, Mutex};

use crate::acp::{runtime::RuntimeHost, scheduler::RuntimeOwner};

mod access;
mod shutdown;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeSlotError {
    #[error("the one live runtime slot is occupied")]
    Occupied,
    #[error("the live runtime belongs to a different session epoch")]
    Foreign,
    #[error("runtime shutdown has started")]
    Stopping,
    #[error("the exact runtime owner requires authoritative recovery")]
    Recovery,
}

pub(super) enum State {
    Empty,
    Starting {
        owner: RuntimeOwner,
    },
    StartingRecovery {
        owner: RuntimeOwner,
    },
    Primed {
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    },
    Claimed {
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    },
    Recovery {
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    },
    Stopping {
        owner: Option<RuntimeOwner>,
        host: Option<Arc<RuntimeHost>>,
    },
    Stopped,
}

pub(crate) struct RuntimeSlot {
    state: Mutex<State>,
    pub(crate) config: Arc<crate::acp::session_config::ConfigChannel>,
}

impl RuntimeSlot {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State::Empty),
            config: Arc::new(crate::acp::session_config::ConfigChannel::default()),
        }
    }

    #[cfg(test)]
    pub(crate) fn claim(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    ) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Empty => {
                *state = State::Claimed { owner, host };
                Ok(())
            }
            State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Occupied),
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
        }
    }

    pub(crate) fn reserve_start(&self, owner: RuntimeOwner) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Empty => {
                self.config.reserve(owner.clone());
                *state = State::Starting { owner };
                Ok(())
            }
            State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Occupied),
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
        }
    }

    pub(crate) fn finish_start(
        &self,
        owner: &RuntimeOwner,
        host: Arc<RuntimeHost>,
    ) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Starting { owner: current } if current == owner => {
                *state = State::Primed {
                    owner: current.clone(),
                    host,
                };
                Ok(())
            }
            State::StartingRecovery { owner: current } if current == owner => {
                Err(RuntimeSlotError::Recovery)
            }
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Foreign),
        }
    }

    /// Publishes a fully started runner to IPC consumers. Until this exact
    /// transition, the primed host is retained for rollback/shutdown but is
    /// deliberately not addressable through `for_owner`.
    pub(crate) fn publish_start(&self, owner: &RuntimeOwner) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Primed {
                owner: current,
                host,
            } if current == owner => {
                *state = State::Claimed {
                    owner: current.clone(),
                    host: Arc::clone(host),
                };
                self.config.publish(owner);
                Ok(())
            }
            State::StartingRecovery { owner: current } | State::Recovery { owner: current, .. }
                if current == owner =>
            {
                Err(RuntimeSlotError::Recovery)
            }
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Foreign),
        }
    }

    pub(crate) fn abandon_start(&self, owner: &RuntimeOwner) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Starting { owner: current } if current == owner => {
                self.config.clear(owner);
                *state = State::Empty;
                Ok(())
            }
            State::StartingRecovery { owner: current } if current == owner => {
                Err(RuntimeSlotError::Recovery)
            }
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Foreign),
        }
    }

    /// Releases a start reservation after the durable activation intent was
    /// rolled back with exact zero-process proof. This is deliberately valid
    /// from StartingRecovery too: the rollback commit is the authoritative
    /// recovery boundary that makes the slot reusable.
    pub(crate) fn abort_start(&self, owner: &RuntimeOwner) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Starting { owner: current } | State::StartingRecovery { owner: current }
                if current == owner =>
            {
                self.config.clear(owner);
                *state = State::Empty;
                Ok(())
            }
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Foreign),
        }
    }

    /// Releases a host installed during activation only after the caller has
    /// joined that exact host and durably rolled the activation back.
    pub(crate) fn abort_claimed_start(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
    ) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Primed {
                owner: current,
                host: current_host,
            }
            | State::Claimed {
                owner: current,
                host: current_host,
            }
            | State::Recovery {
                owner: current,
                host: current_host,
            } if current == owner && Arc::ptr_eq(current_host, host) => {
                self.config.clear(owner);
                *state = State::Empty;
                Ok(())
            }
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Foreign),
        }
    }

    pub(crate) fn for_owner(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<Arc<RuntimeHost>, RuntimeSlotError> {
        match &*self.lock() {
            State::Claimed {
                owner: current,
                host,
            } if current == owner => Ok(Arc::clone(host)),
            State::Recovery { owner: current, .. } if current == owner => {
                Err(RuntimeSlotError::Recovery)
            }
            State::StartingRecovery { owner: current } if current == owner => {
                Err(RuntimeSlotError::Recovery)
            }
            State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. }
            | State::Empty => Err(RuntimeSlotError::Foreign),
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
        }
    }

    pub(crate) fn mark_recovery(&self, owner: &RuntimeOwner) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Starting { owner: current } if current == owner => {
                self.config.clear(owner);
                *state = State::StartingRecovery {
                    owner: current.clone(),
                };
                Ok(())
            }
            State::Primed {
                owner: current,
                host,
            }
            | State::Claimed {
                owner: current,
                host,
            } if current == owner => {
                self.config
                    .remove(owner)
                    .map_err(|_| RuntimeSlotError::Recovery)?;
                *state = State::Recovery {
                    owner: current.clone(),
                    host: Arc::clone(host),
                };
                Ok(())
            }
            State::StartingRecovery { owner: current } if current == owner => Ok(()),
            State::Recovery { owner: current, .. } if current == owner => self
                .config
                .remove(owner)
                .map_err(|_| RuntimeSlotError::Recovery),
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Foreign),
        }
    }

    pub(crate) fn release(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
    ) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Claimed {
                owner: current,
                host: current_host,
            } if current == owner && Arc::ptr_eq(current_host, host) => {
                self.config.clear(owner);
                *state = State::Empty;
                Ok(())
            }
            State::Recovery { owner: current, .. } if current == owner => {
                Err(RuntimeSlotError::Recovery)
            }
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Foreign),
        }
    }

    /// Releases an exact host only after explicit close has converged while
    /// the slot was retaining it for authoritative recovery.
    pub(crate) fn release_recovered(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
    ) -> Result<(), RuntimeSlotError> {
        let mut state = self.lock();
        match &*state {
            State::Recovery {
                owner: current,
                host: current_host,
            } if current == owner && Arc::ptr_eq(current_host, host) => {
                self.config.clear(owner);
                *state = State::Empty;
                Ok(())
            }
            State::Stopping { .. } | State::Stopped => Err(RuntimeSlotError::Stopping),
            State::Empty
            | State::Starting { .. }
            | State::StartingRecovery { .. }
            | State::Primed { .. }
            | State::Claimed { .. }
            | State::Recovery { .. } => Err(RuntimeSlotError::Foreign),
        }
    }

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

    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
