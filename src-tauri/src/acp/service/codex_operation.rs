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

pub(super) trait CodexOperationCancellation: Send + Sync {
    fn request_parent_shutdown(&self) -> Result<(), CodexDiscoveryRunError>;
}

struct TreeCancellation(TreeControl);

impl CodexOperationCancellation for TreeCancellation {
    fn request_parent_shutdown(&self) -> Result<(), CodexDiscoveryRunError> {
        self.0
            .request_termination(TreeExit::ParentShutdown)
            .map_err(|_| CodexDiscoveryRunError::Cleanup)
    }
}

struct AdmissionLease {
    occupied: Arc<AtomicBool>,
}

impl Drop for AdmissionLease {
    fn drop(&mut self) {
        self.occupied.store(false, Ordering::Release);
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

#[derive(Default)]
struct ActiveState {
    running: bool,
    class: Option<OperationClass>,
    cancel_requested: bool,
    cancellation: Option<Arc<dyn CodexOperationCancellation>>,
    settlement: Option<Result<(), CodexDiscoveryRunError>>,
}

pub(super) struct ActiveCodexOperation {
    gate: Arc<CodexOperationGate>,
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
        if !active.running || active.cancellation.is_some() {
            return Err(CodexDiscoveryRunError::Cleanup);
        }
        active.cancellation = Some(cancellation);
        if active.cancel_requested || self.gate.shutting_down.load(Ordering::Acquire) {
            Err(CodexDiscoveryRunError::Drift)
        } else {
            Ok(())
        }
    }

    pub(super) fn settle(mut self, settlement: Result<(), CodexDiscoveryRunError>) {
        self.gate.finish_active(settlement);
        self.settled = true;
    }
}

impl Drop for ActiveCodexOperation {
    fn drop(&mut self) {
        if !self.settled {
            self.gate
                .finish_active(Err(CodexDiscoveryRunError::Cleanup));
        }
    }
}

pub(super) struct CodexOperationGate {
    admission_occupied: Arc<AtomicBool>,
    next_runtime_epoch: AtomicU64,
    invalidation_epoch: AtomicU64,
    shutting_down: AtomicBool,
    active: Mutex<ActiveState>,
    active_changed: Condvar,
}

impl CodexOperationGate {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            admission_occupied: Arc::new(AtomicBool::new(false)),
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
        self.admission_occupied
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| CodexDiscoveryRunError::Busy)?;
        let admission = AdmissionLease {
            occupied: Arc::clone(&self.admission_occupied),
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
        if !self.is_current(prepared.invalidation_epoch) {
            return Err(CodexDiscoveryRunError::Drift);
        }
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(CodexDiscoveryRunError::Unavailable);
        }
        let mut active = self
            .active
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(CodexDiscoveryRunError::Unavailable);
        }
        if active.running {
            return Err(CodexDiscoveryRunError::Busy);
        }
        active.running = true;
        active.class = Some(prepared.class);
        active.cancel_requested = false;
        active.cancellation = None;
        active.settlement = None;
        Ok(ActiveCodexOperation {
            gate: Arc::clone(self),
            settled: false,
        })
    }

    pub(super) fn allocate_runtime_epoch(&self) -> Result<RuntimeEpoch, CodexDiscoveryRunError> {
        let epoch = self.next_runtime_epoch.fetch_add(1, Ordering::AcqRel);
        RuntimeEpoch::new(epoch).map_err(|_| CodexDiscoveryRunError::Transport)
    }

    pub(super) fn is_current(&self, expected: u64) -> bool {
        self.invalidation_epoch.load(Ordering::Acquire) == expected
    }

    pub(super) fn prepared_epoch(prepared: &PreparedCodexOperation) -> u64 {
        prepared.invalidation_epoch
    }

    #[cfg(test)]
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
        cancellation.and(self.wait_for_idle(timeout, was_running))
    }

    pub(super) fn invalidate_ephemeral_and_wait(
        &self,
        timeout: Duration,
    ) -> Result<(), CodexDiscoveryRunError> {
        let (was_running, cancellation) = self.invalidate_ephemeral();
        cancellation.and(self.wait_for_idle(timeout, was_running))
    }

    pub(super) fn shutdown(&self, timeout: Duration) -> Result<(), CodexDiscoveryRunError> {
        self.shutting_down.store(true, Ordering::Release);
        let (was_running, cancellation) = self.invalidate_active();
        cancellation.and(self.wait_for_idle(timeout, was_running))
    }

    fn invalidate_active(&self) -> (bool, Result<(), CodexDiscoveryRunError>) {
        self.invalidate_active_with(|| {})
    }

    fn invalidate_ephemeral(&self) -> (bool, Result<(), CodexDiscoveryRunError>) {
        self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
        let cancellation = match self.active.lock() {
            Ok(mut active) => {
                if !active.running || active.class == Some(OperationClass::Live) {
                    return (false, Ok(()));
                }
                active.cancel_requested = true;
                active.cancellation.clone()
            }
            Err(_) => return (true, Err(CodexDiscoveryRunError::Cleanup)),
        };
        let result = cancellation.map_or(Ok(()), |value| value.request_parent_shutdown());
        (true, result)
    }

    fn invalidate_active_with(
        &self,
        before_active_lock: impl FnOnce(),
    ) -> (bool, Result<(), CodexDiscoveryRunError>) {
        before_active_lock();
        let cancellation = match self.active.lock() {
            Ok(mut active) => {
                self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
                if !active.running {
                    return (false, Ok(()));
                }
                active.cancel_requested = true;
                active.cancellation.clone()
            }
            Err(_) => {
                self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
                return (true, Err(CodexDiscoveryRunError::Cleanup));
            }
        };
        let result = cancellation.map_or(Ok(()), |cancellation| {
            cancellation.request_parent_shutdown()
        });
        (true, result)
    }

    fn wait_for_idle(
        &self,
        timeout: Duration,
        require_settlement: bool,
    ) -> Result<(), CodexDiscoveryRunError> {
        if !require_settlement {
            return Ok(());
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        let mut active = self
            .active
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        while active.running {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CodexDiscoveryRunError::Timeout);
            }
            let (next, wait) = self
                .active_changed
                .wait_timeout(active, remaining)
                .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
            active = next;
            if wait.timed_out() && active.running {
                return Err(CodexDiscoveryRunError::Timeout);
            }
        }
        if require_settlement {
            active
                .settlement
                .take()
                .unwrap_or(Err(CodexDiscoveryRunError::Cleanup))
        } else {
            Ok(())
        }
    }

    fn finish_active(&self, settlement: Result<(), CodexDiscoveryRunError>) {
        match self.active.lock() {
            Ok(mut active) => {
                active.running = false;
                active.class = None;
                active.cancel_requested = false;
                active.cancellation = None;
                active.settlement = Some(settlement);
                self.active_changed.notify_all();
            }
            Err(poisoned) => {
                let mut active = poisoned.into_inner();
                active.running = false;
                active.class = None;
                active.cancellation = None;
                active.settlement = Some(Err(CodexDiscoveryRunError::Cleanup));
                self.active_changed.notify_all();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread};

    struct TestCancellation {
        requested: AtomicBool,
    }

    impl CodexOperationCancellation for TestCancellation {
        fn request_parent_shutdown(&self) -> Result<(), CodexDiscoveryRunError> {
            self.requested.store(true, Ordering::Release);
            Ok(())
        }
    }

    #[test]
    fn admission_is_shared_non_waiting_and_transferable() {
        let gate = CodexOperationGate::new();
        let held = gate.prepare().unwrap();
        let concurrent = Arc::clone(&gate);
        let (send, receive) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || send.send(concurrent.prepare().map(drop)).unwrap());
        assert_eq!(
            receive.recv_timeout(Duration::from_millis(100)),
            Ok(Err(CodexDiscoveryRunError::Busy))
        );
        drop(held);
        worker.join().unwrap();
        let transferable = gate.prepare().unwrap();
        thread::spawn(move || drop(transferable)).join().unwrap();
        assert!(gate.prepare().is_ok());
    }

    #[test]
    fn invalidation_cancels_active_work_and_requires_settlement() {
        let gate = CodexOperationGate::new();
        let prepared = gate.prepare().unwrap();
        let active = gate.begin(&prepared).unwrap();
        let cancellation = Arc::new(TestCancellation {
            requested: AtomicBool::new(false),
        });
        active.register_cancellation(cancellation.clone()).unwrap();
        let invalidating = Arc::clone(&gate);
        let worker =
            thread::spawn(move || invalidating.invalidate_and_wait(Duration::from_secs(1)));
        while !cancellation.requested.load(Ordering::Acquire) {
            thread::yield_now();
        }
        active.settle(Ok(()));
        assert_eq!(worker.join().unwrap(), Ok(()));
        assert!(!gate.is_current(CodexOperationGate::prepared_epoch(&prepared)));
    }

    #[test]
    fn project_switch_invalidates_ephemeral_work_but_preserves_a_live_session() {
        let gate = CodexOperationGate::new();
        let prepared = gate.prepare_live().unwrap();
        let active = gate.begin(&prepared).unwrap();
        let cancellation = Arc::new(TestCancellation {
            requested: AtomicBool::new(false),
        });
        active.register_cancellation(cancellation.clone()).unwrap();

        assert_eq!(
            gate.invalidate_ephemeral_and_wait(Duration::from_millis(10)),
            Ok(())
        );
        assert!(!cancellation.requested.load(Ordering::Acquire));
        assert!(!gate.is_current(CodexOperationGate::prepared_epoch(&prepared)));
        active.settle(Ok(()));
    }
}
