use std::sync::{Arc, Mutex, OnceLock};

use crate::acp::limits::LIMITS;

use super::supervisor::LaunchError;

struct CapacityState {
    live_trees: u32,
    descendants: u32,
}

/// Shared native capacity authority. Only this pool can mint a launch lease.
pub(crate) struct TreeCapacityPool {
    live_tree_limit: u32,
    descendant_limit: u32,
    state: Mutex<CapacityState>,
}

impl TreeCapacityPool {
    pub(crate) fn global() -> Arc<Self> {
        static GLOBAL: OnceLock<Arc<TreeCapacityPool>> = OnceLock::new();
        Arc::clone(GLOBAL.get_or_init(Self::new_ade_0006a))
    }

    fn new_ade_0006a() -> Arc<Self> {
        Arc::new(Self {
            live_tree_limit: u32::try_from(LIMITS.process.operational_live_trees)
                .unwrap_or(u32::MAX),
            descendant_limit: u32::try_from(LIMITS.process.descendants_global).unwrap_or(u32::MAX),
            state: Mutex::new(CapacityState {
                live_trees: 0,
                descendants: 0,
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn isolated_ade_0006a() -> Arc<Self> {
        Self::new_ade_0006a()
    }

    pub(crate) fn try_acquire(
        self: &Arc<Self>,
        descendants: u32,
    ) -> Result<TreeCapacityLease, LaunchError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
        let next_trees = state
            .live_trees
            .checked_add(1)
            .ok_or(LaunchError::CapacityUnavailable)?;
        let next_descendants = state
            .descendants
            .checked_add(descendants)
            .ok_or(LaunchError::CapacityUnavailable)?;
        if next_trees > self.live_tree_limit || next_descendants > self.descendant_limit {
            return Err(LaunchError::CapacityUnavailable);
        }
        state.live_trees = next_trees;
        state.descendants = next_descendants;
        Ok(TreeCapacityLease {
            pool: Arc::clone(self),
            descendants,
            state: LeaseState::Prelaunch,
        })
    }

    fn release(&self, descendants: u32) -> Result<(), LaunchError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
        if state.live_trees == 0 || state.descendants < descendants {
            return Err(LaunchError::ContainmentFailure);
        }
        state.live_trees = state.live_trees.saturating_sub(1);
        state.descendants = state.descendants.saturating_sub(descendants);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn usage(&self) -> (u32, u32) {
        let state = self.state.lock().unwrap();
        (state.live_trees, state.descendants)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeaseState {
    Prelaunch,
    Active,
    Released,
}

/// A non-forgeable reservation for one live tree and its descendant budget.
pub(crate) struct TreeCapacityLease {
    pool: Arc<TreeCapacityPool>,
    descendants: u32,
    state: LeaseState,
}

impl TreeCapacityLease {
    pub(super) fn matches(&self, descendants: u32) -> bool {
        self.descendants == descendants && self.state == LeaseState::Prelaunch
    }

    pub(super) fn activate(&mut self) {
        debug_assert!(self.state == LeaseState::Prelaunch);
        self.state = LeaseState::Active;
    }

    pub(super) fn release_prelaunch_after_zero(&mut self) -> Result<(), LaunchError> {
        if self.state != LeaseState::Prelaunch {
            return Err(LaunchError::ContainmentFailure);
        }
        self.pool.release(self.descendants)?;
        self.state = LeaseState::Released;
        Ok(())
    }

    pub(super) fn release_active_after_zero(&mut self) -> Result<(), LaunchError> {
        if self.state != LeaseState::Active {
            return Err(LaunchError::ContainmentFailure);
        }
        self.pool.release(self.descendants)?;
        self.state = LeaseState::Released;
        Ok(())
    }

    pub(super) fn release_after_zero(mut self) -> Result<(), LaunchError> {
        self.release_active_after_zero()
    }
}

impl Drop for TreeCapacityLease {
    fn drop(&mut self) {
        if self.state == LeaseState::Prelaunch && self.pool.release(self.descendants).is_ok() {
            self.state = LeaseState::Released;
        }
        // An active lease intentionally fails closed if zero-process
        // convergence was not proven: capacity remains unavailable.
    }
}
