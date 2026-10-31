use super::*;
use std::{sync::mpsc, thread};

struct TestCancellation {
    requested: AtomicBool,
}

impl CodexOperationCancellation for TestCancellation {
    fn request_parent_shutdown_until(
        &self,
        _deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError> {
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
    let worker = thread::spawn(move || invalidating.invalidate_and_wait(Duration::from_secs(1)));
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
        gate.invalidate_ephemeral_and_wait_until(Instant::now() + Duration::from_millis(10)),
        Ok(())
    );
    assert!(!cancellation.requested.load(Ordering::Acquire));
    assert!(!gate.is_current(CodexOperationGate::prepared_epoch(&prepared)));
    active.settle(Ok(()));
}

#[test]
fn shutdown_does_not_wait_for_a_live_operation() {
    let gate = CodexOperationGate::new();
    let prepared = gate.prepare_live().unwrap();
    let active = gate.begin(&prepared).unwrap();
    let cancellation = Arc::new(TestCancellation {
        requested: AtomicBool::new(false),
    });
    active.register_cancellation(cancellation.clone()).unwrap();

    assert_eq!(gate.shutdown(Duration::from_millis(10)), Ok(()));
    assert!(!cancellation.requested.load(Ordering::Acquire));
    active.settle(Ok(()));
}

#[test]
fn live_operations_run_concurrently_up_to_the_lane_count_and_fill_capacity() {
    let gate = CodexOperationGate::new();
    let mut live = Vec::new();
    for _ in 0..LIVE_OPERATIONS {
        let prepared = gate.prepare_live().unwrap();
        live.push(gate.begin(&prepared).unwrap());
    }
    assert!(matches!(
        gate.begin(&gate.prepare_live().unwrap()),
        Err(CodexDiscoveryRunError::Busy)
    ));
    // Full capacity rejects ephemeral work at begin.
    let ephemeral = gate.prepare().unwrap();
    assert!(matches!(
        gate.begin(&ephemeral),
        Err(CodexDiscoveryRunError::Busy)
    ));
    drop(ephemeral);
    live.pop().unwrap().settle(Ok(()));
    // A held live admission plus running work at capacity rejects admission.
    let replacement = gate.prepare_live().unwrap();
    live.push(gate.begin(&replacement).unwrap());
    let held: Vec<_> = (1..LIVE_OPERATIONS)
        .map(|_| gate.prepare_live().unwrap())
        .collect();
    assert!(matches!(gate.prepare(), Err(CodexDiscoveryRunError::Busy)));
    drop(held);
    drop(replacement);
    for operation in live {
        operation.settle(Ok(()));
    }
    let ephemeral = gate.prepare().unwrap();
    gate.begin(&ephemeral).unwrap().settle(Ok(()));
}

#[test]
fn ephemeral_discovery_is_admitted_beside_live_sessions_with_spare_capacity() {
    let gate = CodexOperationGate::new();
    // A live session (Claude, Pi, or a retained completed one) holds a lane.
    let live_prepared = gate.prepare_live().unwrap();
    let live = gate.begin(&live_prepared).unwrap();
    let ephemeral = gate.prepare().unwrap();
    let discovery = gate.begin(&ephemeral).unwrap();
    // Only one ephemeral operation at a time.
    assert!(matches!(gate.prepare(), Err(CodexDiscoveryRunError::Busy)));
    // Live work fills the remaining lanes and then stops at capacity.
    let mut more = Vec::new();
    for _ in 2..LIVE_OPERATIONS {
        let prepared = gate.prepare_live().unwrap();
        more.push(gate.begin(&prepared).unwrap());
    }
    assert!(matches!(
        gate.begin(&gate.prepare_live().unwrap()),
        Err(CodexDiscoveryRunError::Busy)
    ));
    discovery.settle(Ok(()));
    live.settle(Ok(()));
    for operation in more {
        operation.settle(Ok(()));
    }
}

#[test]
fn binding_invalidation_cancels_every_live_operation() {
    let gate = CodexOperationGate::new();
    let cancellations: Vec<_> = (0..2)
        .map(|_| {
            Arc::new(TestCancellation {
                requested: AtomicBool::new(false),
            })
        })
        .collect();
    let live: Vec<_> = cancellations
        .iter()
        .map(|cancellation| {
            let prepared = gate.prepare_live().unwrap();
            let active = gate.begin(&prepared).unwrap();
            active.register_cancellation(cancellation.clone()).unwrap();
            active
        })
        .collect();
    gate.invalidate();
    assert!(
        cancellations
            .iter()
            .all(|cancellation| cancellation.requested.load(Ordering::Acquire))
    );
    for operation in live {
        operation.settle(Ok(()));
    }
}

#[test]
fn ephemeral_waiters_keep_their_own_settlement_when_live_work_finishes() {
    struct SettleBoth(Mutex<Option<(ActiveCodexOperation, ActiveCodexOperation)>>);
    impl CodexOperationCancellation for SettleBoth {
        fn request_parent_shutdown_until(&self, _: Instant) -> Result<(), CodexDiscoveryRunError> {
            let (ephemeral, live) = self.0.lock().unwrap().take().unwrap();
            ephemeral.settle(Ok(()));
            live.settle(Err(CodexDiscoveryRunError::Cleanup));
            Ok(())
        }
    }
    let gate = CodexOperationGate::new();
    let live_prepared = gate.prepare_live().unwrap();
    let live = gate.begin(&live_prepared).unwrap();
    let prepared = gate.prepare().unwrap();
    let ephemeral = gate.begin(&prepared).unwrap();
    let cancellation = Arc::new(SettleBoth(Mutex::new(None)));
    ephemeral
        .register_cancellation(cancellation.clone())
        .unwrap();
    *cancellation.0.lock().unwrap() = Some((ephemeral, live));
    assert_eq!(
        gate.invalidate_ephemeral_and_wait_until(Instant::now() + Duration::from_secs(5)),
        Ok(())
    );
}
