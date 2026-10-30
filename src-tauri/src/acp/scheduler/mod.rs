use std::sync::{Arc, Mutex};

use crate::acp::contract::{
    RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeOwner {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveAllocation {
    pub tree: RuntimeOwner,
    pub turn_id: Option<SageTurnId>,
    pub turn_reserved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum SchedulerError {
    #[error("the one-tree operational slot is occupied")]
    TreeCapacity,
    #[error("the one-running-Turn operational slot is occupied")]
    TurnCapacity,
    #[error("the runtime owner or epoch is stale")]
    StaleOwner,
    #[error("the monotonic scheduler token is stale")]
    StaleToken,
    #[error("the monotonic scheduler token is exhausted")]
    TokenExhausted,
    #[error("a Turn still owns the tree")]
    TurnStillActive,
}

#[derive(Debug, Clone)]
pub(crate) struct Scheduler {
    inner: Arc<Mutex<State>>,
}

#[derive(Debug)]
struct State {
    next_token: u64,
    tree: Option<TreeSlot>,
    turn: Option<TurnSlot>,
}

#[derive(Debug)]
struct TreeSlot {
    token: u64,
    owner: RuntimeOwner,
    draining: bool,
}

#[derive(Debug)]
struct TurnSlot {
    token: u64,
    tree_token: u64,
    turn_id: Option<SageTurnId>,
}

#[derive(Debug)]
pub(crate) struct TreeLease {
    inner: Arc<Mutex<State>>,
    token: u64,
    owner: RuntimeOwner,
    converged: bool,
}

#[derive(Debug)]
pub(crate) struct TurnReservation {
    inner: Arc<Mutex<State>>,
    token: u64,
    tree_token: u64,
    completed: bool,
}

#[derive(Debug)]
pub(crate) struct RunningTurnLease {
    inner: Arc<Mutex<State>>,
    token: u64,
    tree_token: u64,
    turn_id: SageTurnId,
    terminal_committed: bool,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(State {
                next_token: 1,
                tree: None,
                turn: None,
            })),
        }
    }

    pub(crate) fn reserve_tree(&self, owner: RuntimeOwner) -> Result<TreeLease, SchedulerError> {
        let mut state = self.lock();
        if state.tree.is_some() {
            return Err(SchedulerError::TreeCapacity);
        }
        let token = take_token(&mut state)?;
        state.tree = Some(TreeSlot {
            token,
            owner: owner.clone(),
            draining: false,
        });
        drop(state);
        Ok(TreeLease {
            inner: Arc::clone(&self.inner),
            token,
            owner,
            converged: false,
        })
    }

    pub(crate) fn active(&self) -> Option<ActiveAllocation> {
        let state = self.lock();
        state.tree.as_ref().map(|tree| ActiveAllocation {
            tree: tree.owner.clone(),
            turn_id: state.turn.as_ref().and_then(|turn| turn.turn_id.clone()),
            turn_reserved: state.turn.is_some(),
        })
    }

    pub(crate) fn begin_idle_drain(&self, owner: &RuntimeOwner) -> Result<(), SchedulerError> {
        let mut state = self.lock();
        if state.turn.is_some() {
            return Err(SchedulerError::TurnStillActive);
        }
        let Some(tree) = state.tree.as_mut() else {
            return Err(SchedulerError::StaleToken);
        };
        if &tree.owner != owner {
            return Err(SchedulerError::StaleOwner);
        }
        if tree.draining {
            return Err(SchedulerError::TreeCapacity);
        }
        tree.draining = true;
        Ok(())
    }

    pub(crate) fn cancel_idle_drain(&self, owner: &RuntimeOwner) -> Result<(), SchedulerError> {
        let mut state = self.lock();
        if state.turn.is_some() {
            return Err(SchedulerError::TurnStillActive);
        }
        let Some(tree) = state.tree.as_mut() else {
            return Err(SchedulerError::StaleToken);
        };
        if &tree.owner != owner || !tree.draining {
            return Err(SchedulerError::StaleOwner);
        }
        tree.draining = false;
        Ok(())
    }

    pub(crate) fn project_switched(&self, _active_project: &SageProjectId) {
        // A project switch changes only the disposable projection. A running
        // Turn and its tree retain both global slots in the background.
    }

    /// Releases the exact scheduler tree after durable terminalization and a
    /// native zero-process proof. This is the only recovery path allowed to
    /// clear an active Turn together with its failed runtime tree.
    pub(crate) fn recover_owner_after_zero_process(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<(), SchedulerError> {
        let mut state = self.lock();
        let Some(tree) = state.tree.as_ref() else {
            return Ok(());
        };
        if &tree.owner != owner {
            return Err(SchedulerError::StaleOwner);
        }
        if state
            .turn
            .as_ref()
            .is_some_and(|turn| turn.tree_token != tree.token)
        {
            return Err(SchedulerError::StaleToken);
        }
        state.turn = None;
        state.tree = None;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl TreeLease {
    pub(crate) fn owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    pub(crate) fn reserve_turn(&self) -> Result<TurnReservation, SchedulerError> {
        let mut state = lock(&self.inner);
        let tree = state.tree.as_ref().ok_or(SchedulerError::StaleToken)?;
        if tree.token != self.token || tree.owner != self.owner {
            return Err(SchedulerError::StaleOwner);
        }
        if tree.draining {
            return Err(SchedulerError::TreeCapacity);
        }
        if state.turn.is_some() {
            return Err(SchedulerError::TurnCapacity);
        }
        let token = take_token(&mut state)?;
        state.turn = Some(TurnSlot {
            token,
            tree_token: self.token,
            turn_id: None,
        });
        drop(state);
        Ok(TurnReservation {
            inner: Arc::clone(&self.inner),
            token,
            tree_token: self.token,
            completed: false,
        })
    }

    /// Stops new Turn reservations after the durable session-close intent has
    /// committed. An already-owned Turn remains charged until its own durable
    /// terminal, allowing cancellation/teardown to converge in order.
    pub(crate) fn begin_draining(&self) -> Result<(), SchedulerError> {
        let mut state = lock(&self.inner);
        let tree = state.tree.as_mut().ok_or(SchedulerError::StaleToken)?;
        if tree.token != self.token || tree.owner != self.owner {
            return Err(SchedulerError::StaleOwner);
        }
        tree.draining = true;
        Ok(())
    }

    pub(crate) fn zero_process_converged(self) -> Result<(), SchedulerError> {
        match self.zero_process_converged_retained() {
            Ok(()) => Ok(()),
            Err((_lease, error)) => Err(error),
        }
    }

    pub(crate) fn zero_process_converged_retained(mut self) -> Result<(), (Self, SchedulerError)> {
        let mut state = lock(&self.inner);
        let Some(tree) = state.tree.as_ref() else {
            drop(state);
            return Err((self, SchedulerError::StaleToken));
        };
        if tree.token != self.token || tree.owner != self.owner {
            drop(state);
            return Err((self, SchedulerError::StaleOwner));
        }
        if state.turn.is_some() {
            drop(state);
            return Err((self, SchedulerError::TurnStillActive));
        }
        state.tree = None;
        self.converged = true;
        Ok(())
    }
}

impl Drop for TreeLease {
    fn drop(&mut self) {
        // Unexpected loss of native ownership does not advertise capacity.
        // Recovery/teardown must prove zero-process convergence explicitly.
        let _ = self.converged;
    }
}

impl TurnReservation {
    #[cfg(test)]
    pub(crate) fn bind(self, turn_id: SageTurnId) -> Result<RunningTurnLease, SchedulerError> {
        match self.bind_retained(turn_id) {
            Ok(lease) => Ok(lease),
            Err((_reservation, error)) => Err(error),
        }
    }

    pub(crate) fn bind_retained(
        mut self,
        turn_id: SageTurnId,
    ) -> Result<RunningTurnLease, (Self, SchedulerError)> {
        let mut state = lock(&self.inner);
        let Some(turn) = state.turn.as_mut() else {
            drop(state);
            return Err((self, SchedulerError::StaleToken));
        };
        if turn.token != self.token || turn.tree_token != self.tree_token {
            drop(state);
            return Err((self, SchedulerError::StaleToken));
        }
        if turn.turn_id.is_some() {
            drop(state);
            return Err((self, SchedulerError::TurnCapacity));
        }
        turn.turn_id = Some(turn_id.clone());
        drop(state);
        self.completed = true;
        Ok(RunningTurnLease {
            inner: Arc::clone(&self.inner),
            token: self.token,
            tree_token: self.tree_token,
            turn_id,
            terminal_committed: false,
        })
    }

    pub(crate) fn abort_before_durable_commit(mut self) -> Result<(), SchedulerError> {
        clear_turn(&self.inner, self.token, self.tree_token, None)?;
        self.completed = true;
        Ok(())
    }
}

impl Drop for TurnReservation {
    fn drop(&mut self) {
        // An unexplained drop may follow an unknown commit outcome, so the
        // reservation remains charged until explicit recovery.
        let _ = self.completed;
    }
}

impl RunningTurnLease {
    pub(crate) fn terminal_committed(mut self) -> Result<(), SchedulerError> {
        let mut state = lock(&self.inner);
        let tree = state.tree.as_ref().ok_or(SchedulerError::StaleToken)?;
        if tree.token != self.tree_token {
            return Err(SchedulerError::StaleToken);
        }
        clear_turn_in(
            &mut state,
            &self.inner,
            self.token,
            self.tree_token,
            Some(&self.turn_id),
        )?;
        self.terminal_committed = true;
        Ok(())
    }
}

impl Drop for RunningTurnLease {
    fn drop(&mut self) {
        // Capacity remains charged unless a durable terminal was committed.
        let _ = self.terminal_committed;
    }
}

fn clear_turn(
    inner: &Mutex<State>,
    token: u64,
    tree_token: u64,
    turn_id: Option<&SageTurnId>,
) -> Result<(), SchedulerError> {
    let mut state = lock(inner);
    clear_turn_in(&mut state, inner, token, tree_token, turn_id)
}

fn clear_turn_in(
    state: &mut State,
    _inner: &Mutex<State>,
    token: u64,
    tree_token: u64,
    turn_id: Option<&SageTurnId>,
) -> Result<(), SchedulerError> {
    let turn = state.turn.as_ref().ok_or(SchedulerError::StaleToken)?;
    if turn.token != token || turn.tree_token != tree_token {
        return Err(SchedulerError::StaleToken);
    }
    match (&turn.turn_id, turn_id) {
        (None, None) => {}
        (Some(current), Some(expected)) if current == expected => {}
        _ => return Err(SchedulerError::StaleToken),
    }
    state.turn = None;
    Ok(())
}

fn take_token(state: &mut State) -> Result<u64, SchedulerError> {
    let token = state.next_token;
    state.next_token = state
        .next_token
        .checked_add(1)
        .filter(|next| *next <= i64::MAX as u64)
        .ok_or(SchedulerError::TokenExhausted)?;
    Ok(token)
}

fn lock(inner: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    inner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
