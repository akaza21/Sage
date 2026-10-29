use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use crate::acp::{
    bindings::codex::CodexDiscoveryRunError,
    contract::RuntimeEpoch,
    process::{TreeControl, TreeExit},
};

/// Live harness operations admitted at once: one per live runtime lane.
const LIVE_OPERATIONS: usize = super::LIVE_RUNTIME_LANES;

mod admission;
use admission::{AdmissionLease, AdmissionState};

pub(super) trait CodexOperationCancellation: Send + Sync {
    fn request_parent_shutdown_until(
        &self,
        deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError>;
}

struct TreeCancellation(TreeControl);

impl CodexOperationCancellation for TreeCancellation {
    fn request_parent_shutdown_until(
        &self,
        deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError> {
        if Instant::now() >= deadline {
            return Err(CodexDiscoveryRunError::Timeout);
        }
        self.0
            .request_termination(TreeExit::ParentShutdown)
            .map_err(|_| CodexDiscoveryRunError::Cleanup)
    }
}

pub(super) struct PreparedCodexOperation {
    invalidation_epoch: u64,
    class: OperationClass,
    _admission: AdmissionLease,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OperationClass {
    Ephemeral,
    Live,
}

struct OperationSlot {
    id: u64,
    class: OperationClass,
    cancel_requested: bool,
    cancellation: Option<Arc<dyn CodexOperationCancellation>>,
}

#[derive(Default)]
struct ActiveState {
    next_id: u64,
    // Bounded: at most `LIVE_OPERATIONS` operations, at most one ephemeral.
    running: Vec<OperationSlot>,
    settlement: Option<Result<(), CodexDiscoveryRunError>>,
}

impl ActiveState {
    fn has(&self, class: OperationClass) -> bool {
        self.running.iter().any(|slot| slot.class == class)
    }

    fn slot_mut(&mut self, id: u64) -> Option<&mut OperationSlot> {
        self.running.iter_mut().find(|slot| slot.id == id)
    }
}

pub(super) struct ActiveCodexOperation {
    gate: Arc<CodexOperationGate>,
    id: u64,
    settled: bool,
}

impl ActiveCodexOperation {
    pub(super) fn register_control(
        &self,
        control: TreeControl,
    ) -> Result<(), CodexDiscoveryRunError> {
        self.register_cancellation(Arc::new(TreeCancellation(control)))
    }

    pub(super) fn register_cancellation(
        &self,
        cancellation: Arc<dyn CodexOperationCancellation>,
    ) -> Result<(), CodexDiscoveryRunError> {
        let mut active = self
            .gate
            .active
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        let slot = active
            .slot_mut(self.id)
            .filter(|slot| slot.cancellation.is_none())
            .ok_or(CodexDiscoveryRunError::Cleanup)?;
        slot.cancellation = Some(cancellation);
        if slot.cancel_requested || self.gate.shutting_down.load(Ordering::Acquire) {
            Err(CodexDiscoveryRunError::Drift)
        } else {
            Ok(())
        }
    }

    pub(super) fn settle(mut self, settlement: Result<(), CodexDiscoveryRunError>) {
        self.gate.finish_active(self.id, settlement);
        self.settled = true;
    }
}

impl Drop for ActiveCodexOperation {
    fn drop(&mut self) {
        if !self.settled {
            self.gate
                .finish_active(self.id, Err(CodexDiscoveryRunError::Cleanup));
        }
    }
}

pub(super) struct CodexOperationGate {
    admission: Arc<Mutex<AdmissionState>>,
    next_runtime_epoch: AtomicU64,
    invalidation_epoch: AtomicU64,
    shutting_down: AtomicBool,
    active: Mutex<ActiveState>,
    active_changed: Condvar,
}

impl CodexOperationGate {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            admission: Arc::new(Mutex::new(AdmissionState::default())),
            next_runtime_epoch: AtomicU64::new(1),
            invalidation_epoch: AtomicU64::new(1),
            shutting_down: AtomicBool::new(false),
            active: Mutex::new(ActiveState::default()),
            active_changed: Condvar::new(),
        })
    }

    pub(super) fn prepare(&self) -> Result<PreparedCodexOperation, CodexDiscoveryRunError> {
        self.prepare_class(OperationClass::Ephemeral)
    }

    pub(super) fn prepare_live(&self) -> Result<PreparedCodexOperation, CodexDiscoveryRunError> {
        self.prepare_class(OperationClass::Live)
    }

    fn prepare_class(
        &self,
        class: OperationClass,
    ) -> Result<PreparedCodexOperation, CodexDiscoveryRunError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(CodexDiscoveryRunError::Unavailable);
        }
        {
            let mut admission = self
                .admission
                .lock()
                .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
            let admitted = admission.occupied() < LIVE_OPERATIONS
                && (class == OperationClass::Live || !admission.ephemeral);
            if !admitted {
                return Err(CodexDiscoveryRunError::Busy);
            }
            match class {
                OperationClass::Ephemeral => admission.ephemeral = true,
                OperationClass::Live => admission.live += 1,
            }
        }
        let admission = AdmissionLease {
            admission: Arc::clone(&self.admission),
            class,
        };
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(CodexDiscoveryRunError::Unavailable);
        }
        Ok(PreparedCodexOperation {
            invalidation_epoch: self.invalidation_epoch.load(Ordering::Acquire),
            class,
            _admission: admission,
        })
    }

    pub(super) fn begin(
        self: &Arc<Self>,
        prepared: &PreparedCodexOperation,
    ) -> Result<ActiveCodexOperation, CodexDiscoveryRunError> {
        self.validate_admission(prepared)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(CodexDiscoveryRunError::Unavailable);
        }
        // Capacity, not class, bounds admission: ephemeral work (discovery,
        // sign-in) runs beside live sessions whenever a lane is spare.
        let admitted = active.running.len() < LIVE_OPERATIONS
            && (prepared.class == OperationClass::Live || !active.has(OperationClass::Ephemeral));
        if !admitted {
            return Err(CodexDiscoveryRunError::Busy);
        }
        let id = active.next_id;
        active.next_id = active.next_id.wrapping_add(1);
        active.running.push(OperationSlot {
            id,
            class: prepared.class,
            cancel_requested: false,
            cancellation: None,
        });
        active.settlement = None;
        Ok(ActiveCodexOperation {
            gate: Arc::clone(self),
            id,
            settled: false,
        })
    }

    pub(super) fn allocate_runtime_epoch(&self) -> Result<RuntimeEpoch, CodexDiscoveryRunError> {
        let epoch = self.next_runtime_epoch.fetch_add(1, Ordering::AcqRel);
        RuntimeEpoch::new(epoch).map_err(|_| CodexDiscoveryRunError::Transport)
    }

    pub(super) fn validate_admission(
        &self,
        prepared: &PreparedCodexOperation,
    ) -> Result<(), CodexDiscoveryRunError> {
        self.validate_epoch(prepared.invalidation_epoch)
    }

    pub(super) fn validate_epoch(&self, expected: u64) -> Result<(), CodexDiscoveryRunError> {
        if !self.is_current(expected) {
            return Err(CodexDiscoveryRunError::Drift);
        }
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(CodexDiscoveryRunError::Unavailable);
        }
        Ok(())
    }

    pub(super) fn is_current(&self, expected: u64) -> bool {
        self.invalidation_epoch.load(Ordering::Acquire) == expected
    }

    pub(super) fn prepared_epoch(prepared: &PreparedCodexOperation) -> u64 {
        prepared.invalidation_epoch
    }

    pub(super) fn current_epoch(&self) -> u64 {
        self.invalidation_epoch.load(Ordering::Acquire)
    }

    pub(super) fn invalidate(&self) {
        let _ = self.invalidate_active().1;
    }

    #[cfg(test)]
    pub(super) fn invalidate_and_wait(
        &self,
        timeout: Duration,
    ) -> Result<(), CodexDiscoveryRunError> {
        let (was_running, cancellation) = self.invalidate_active();
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        cancellation.and(self.wait_until(deadline, was_running, None))
    }

    /// Project replacement owns this absolute deadline before cancellation is
    /// dispatched. Invalidation and idle settlement both consume that one
    /// budget; no later layer may mint a fresh relative timeout.
    pub(super) fn invalidate_ephemeral_and_wait_until(
        &self,
        deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError> {
        if Instant::now() >= deadline {
            return Err(CodexDiscoveryRunError::Timeout);
        }
        self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
        let (was_running, cancellation) = {
            let mut active = self
                .active
                .lock()
                .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
            Self::request_ephemeral_cancel(&mut active)
        };
        // Cancellation may call back into gate observation, so it must run
        // after releasing `active` and under the same caller-owned deadline.
        if let Some(cancellation) = cancellation {
            cancellation.request_parent_shutdown_until(deadline)?;
        }
        self.wait_until(deadline, was_running, Some(OperationClass::Ephemeral))
    }

    pub(super) fn shutdown(&self, timeout: Duration) -> Result<(), CodexDiscoveryRunError> {
        self.shutting_down.store(true, Ordering::Release);
        // Live sessions are parked by runtime shutdown. Waiting on them here
        // spends the whole close budget before park can run.
        let (was_running, cancellation) = self.invalidate_ephemeral();
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        cancellation.and(self.wait_until(deadline, was_running, Some(OperationClass::Ephemeral)))
    }

    fn request_ephemeral_cancel(
        active: &mut ActiveState,
    ) -> (bool, Option<Arc<dyn CodexOperationCancellation>>) {
        match active
            .running
            .iter_mut()
            .find(|slot| slot.class == OperationClass::Ephemeral)
        {
            Some(slot) => {
                slot.cancel_requested = true;
                (true, slot.cancellation.clone())
            }
            None => (false, None),
        }
    }

    fn invalidate_ephemeral(&self) -> (bool, Result<(), CodexDiscoveryRunError>) {
        self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
        let (was_running, cancellation) = match self.active.lock() {
            Ok(mut active) => Self::request_ephemeral_cancel(&mut active),
            Err(_) => return (true, Err(CodexDiscoveryRunError::Cleanup)),
        };
        let deadline = Instant::now() + Duration::from_millis(1);
        let result = cancellation.map_or(Ok(()), |value| {
            value.request_parent_shutdown_until(deadline)
        });
        (was_running, result)
    }

    /// Invalidates the admitted binding: every running operation, ephemeral
    /// or live, is asked to stop.
    fn invalidate_active(&self) -> (bool, Result<(), CodexDiscoveryRunError>) {
        let cancellations: Vec<_> = match self.active.lock() {
            Ok(mut active) => {
                self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
                if active.running.is_empty() {
                    return (false, Ok(()));
                }
                active
                    .running
                    .iter_mut()
                    .map(|slot| {
                        slot.cancel_requested = true;
                        slot.cancellation.clone()
                    })
                    .collect()
            }
            Err(_) => {
                self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
                return (true, Err(CodexDiscoveryRunError::Cleanup));
            }
        };
        let deadline = Instant::now() + Duration::from_millis(1);
        let result = cancellations
            .into_iter()
            .flatten()
            .map(|cancellation| cancellation.request_parent_shutdown_until(deadline))
            .fold(Ok(()), Result::and);
        (true, result)
    }

    /// Waits until no operation of `class` (any class when `None`) runs.
    fn wait_until(
        &self,
        deadline: Instant,
        require_settlement: bool,
        class: Option<OperationClass>,
    ) -> Result<(), CodexDiscoveryRunError> {
        if !require_settlement {
            return Ok(());
        }
        let busy = |active: &ActiveState| match class {
            Some(class) => active.has(class),
            None => !active.running.is_empty(),
        };
        let mut active = self
            .active
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        while busy(&active) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CodexDiscoveryRunError::Timeout);
            }
            let (next, wait) = self
                .active_changed
                .wait_timeout(active, remaining)
                .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
            active = next;
            if wait.timed_out() && busy(&active) {
                return Err(CodexDiscoveryRunError::Timeout);
            }
        }
        active
            .settlement
            .take()
            .unwrap_or(Err(CodexDiscoveryRunError::Cleanup))
    }

    fn finish_active(&self, id: u64, settlement: Result<(), CodexDiscoveryRunError>) {
        let mut active = match self.active.lock() {
            Ok(active) => active,
            Err(poisoned) => {
                let mut active = poisoned.into_inner();
                active.running.retain(|slot| slot.id != id);
                active.settlement = Some(Err(CodexDiscoveryRunError::Cleanup));
                self.active_changed.notify_all();
                return;
            }
        };
        active.running.retain(|slot| slot.id != id);
        active.settlement = Some(settlement);
        self.active_changed.notify_all();
    }
}

#[cfg(test)]
#[path = "codex_operation_tests.rs"]
mod tests;
