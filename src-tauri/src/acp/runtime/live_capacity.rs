use std::sync::{Arc, Mutex};

use crate::acp::contract::SageAgentSessionId;
use sage_core::limits::LIMITS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapacityScope {
    Session,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveCapacityError {
    Capacity { scope: CapacityScope, maximum: u64 },
    InvalidCount,
    State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapacityKind {
    Rpc,
    ServerRequest,
    RuntimeTask,
}

struct State {
    owners: Vec<(SageAgentSessionId, u64)>,
    global: u64,
}

pub(crate) struct LiveCapacityPool {
    per_session: u64,
    global: u64,
    state: Mutex<State>,
}

impl LiveCapacityPool {
    #[cfg(test)]
    fn new(kind: CapacityKind) -> Arc<Self> {
        let (per_session, global) = match kind {
            CapacityKind::Rpc => (
                LIMITS.queues.outstanding_rpc_per_session,
                LIMITS.queues.outstanding_rpc_global,
            ),
            CapacityKind::ServerRequest => (
                LIMITS.queues.server_requests_per_session,
                LIMITS.queues.server_requests_global,
            ),
            CapacityKind::RuntimeTask => (
                LIMITS.queues.runtime_tasks_per_session,
                LIMITS.queues.runtime_tasks_global,
            ),
        };
        Arc::new(Self {
            per_session,
            global,
            state: Mutex::new(State {
                owners: Vec::new(),
                global: 0,
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn isolated_rpc() -> Arc<Self> {
        Self::new(CapacityKind::Rpc)
    }

    #[cfg(test)]
    pub(crate) fn isolated_server_requests() -> Arc<Self> {
        Self::new(CapacityKind::ServerRequest)
    }

    #[cfg(test)]
    pub(crate) fn isolated_runtime_tasks() -> Arc<Self> {
        Self::new(CapacityKind::RuntimeTask)
    }

    #[cfg(test)]
    pub(crate) fn try_reserve(
        self: &Arc<Self>,
        owner: SageAgentSessionId,
        count: u64,
    ) -> Result<LiveCapacityLease, LiveCapacityError> {
        if count == 0 {
            return Err(LiveCapacityError::InvalidCount);
        }
        let mut state = self.state.lock().map_err(|_| LiveCapacityError::State)?;
        let existing = state
            .owners
            .iter()
            .position(|(candidate, _)| candidate == &owner);
        let owner_count = existing.map_or(0, |index| state.owners[index].1);
        let next_owner = owner_count
            .checked_add(count)
            .ok_or(LiveCapacityError::Capacity {
                scope: CapacityScope::Session,
                maximum: self.per_session,
            })?;
        if next_owner > self.per_session {
            return Err(LiveCapacityError::Capacity {
                scope: CapacityScope::Session,
                maximum: self.per_session,
            });
        }
        let next_global = state
            .global
            .checked_add(count)
            .ok_or(LiveCapacityError::Capacity {
                scope: CapacityScope::Global,
                maximum: self.global,
            })?;
        if next_global > self.global {
            return Err(LiveCapacityError::Capacity {
                scope: CapacityScope::Global,
                maximum: self.global,
            });
        }
        if let Some(index) = existing {
            state.owners[index].1 = next_owner;
        } else {
            state
                .owners
                .try_reserve_exact(1)
                .map_err(|_| LiveCapacityError::State)?;
            state.owners.push((owner.clone(), count));
        }
        state.global = next_global;
        Ok(LiveCapacityLease {
            pool: Arc::clone(self),
            owner,
            count,
            retained: true,
        })
    }

    #[cfg(test)]
    fn release(&self, owner: &SageAgentSessionId, count: u64) -> Result<(), LiveCapacityError> {
        let mut state = self.state.lock().map_err(|_| LiveCapacityError::State)?;
        let index = state
            .owners
            .iter()
            .position(|(candidate, _)| candidate == owner)
            .ok_or(LiveCapacityError::State)?;
        if state.owners[index].1 < count || state.global < count {
            return Err(LiveCapacityError::State);
        }
        state.owners[index].1 -= count;
        state.global -= count;
        if state.owners[index].1 == 0 {
            state.owners.remove(index);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn usage(&self) -> (usize, u64) {
        let state = self.state.lock().unwrap();
        (state.owners.len(), state.global)
    }
}

pub(crate) struct LiveCapacityLease {
    pool: Arc<LiveCapacityPool>,
    owner: SageAgentSessionId,
    count: u64,
    retained: bool,
}

impl Drop for LiveCapacityLease {
    fn drop(&mut self) {
        if self.retained && self.pool.release(&self.owner, self.count).is_ok() {
            self.retained = false;
        }
    }
}
