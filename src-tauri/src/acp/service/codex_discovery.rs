use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use crate::acp::{
    bindings::codex::{
        CodexBindingRegistration, CodexDiscoveryDeadline, CodexDiscoveryRunError,
        CodexProjectRootAuthority, discover_admitted,
    },
    contract::RuntimeEpoch,
    ipc::{CodexDiscoveryResponse, GetCodexDiscoveryRequest},
    process::{TreeControl, TreeExit},
};

mod cache;
use cache::{CacheKey, CacheScope, CachedDiscovery, response_from, validate_scope};

/// Opaque authority prepared while the active-Project transition is held.
/// Discovery must reject it if any later transition invalidates that moment.
pub(crate) struct PreparedCodexDiscovery {
    request: GetCodexDiscoveryRequest,
    project_root: CodexProjectRootAuthority,
    invalidation_epoch: u64,
    deadline: CodexDiscoveryDeadline,
    _admission: DiscoveryAdmissionLease,
}

struct DiscoveryAdmissionLease {
    occupied: Arc<AtomicBool>,
}

impl Drop for DiscoveryAdmissionLease {
    fn drop(&mut self) {
        self.occupied.store(false, Ordering::Release);
    }
}

#[derive(Default)]
struct ActiveDiscoveryState {
    running: bool,
    cancel_requested: bool,
    cancellation: Option<Arc<dyn DiscoveryCancellation>>,
    settlement: Option<Result<(), CodexDiscoveryRunError>>,
}

trait DiscoveryCancellation: Send + Sync {
    fn request_parent_shutdown(&self) -> Result<(), CodexDiscoveryRunError>;
}

struct TreeDiscoveryCancellation(TreeControl);

impl DiscoveryCancellation for TreeDiscoveryCancellation {
    fn request_parent_shutdown(&self) -> Result<(), CodexDiscoveryRunError> {
        self.0
            .request_termination(TreeExit::ParentShutdown)
            .map_err(|_| CodexDiscoveryRunError::Cleanup)
    }
}

struct ActiveDiscoveryGuard<'a> {
    manager: &'a CodexDiscoveryManager,
    settled: bool,
}

impl ActiveDiscoveryGuard<'_> {
    fn register_control(&self, control: TreeControl) -> Result<(), CodexDiscoveryRunError> {
        self.register_cancellation(Arc::new(TreeDiscoveryCancellation(control)))
    }

    fn register_cancellation(
        &self,
        cancellation: Arc<dyn DiscoveryCancellation>,
    ) -> Result<(), CodexDiscoveryRunError> {
        let mut active = self
            .manager
            .active
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        if !active.running || active.cancellation.is_some() {
            return Err(CodexDiscoveryRunError::Cleanup);
        }
        active.cancellation = Some(cancellation);
        if active.cancel_requested || self.manager.shutting_down.load(Ordering::Acquire) {
            Err(CodexDiscoveryRunError::Drift)
        } else {
            Ok(())
        }
    }

    fn settle(mut self, settlement: Result<(), CodexDiscoveryRunError>) {
        self.manager.finish_active(settlement);
        self.settled = true;
    }
}

impl Drop for ActiveDiscoveryGuard<'_> {
    fn drop(&mut self) {
        if !self.settled {
            self.manager
                .finish_active(Err(CodexDiscoveryRunError::Cleanup));
        }
    }
}

pub(crate) struct CodexDiscoveryManager {
    registration: CodexBindingRegistration,
    admission_occupied: Arc<AtomicBool>,
    cache: Mutex<Option<CachedDiscovery>>,
    next_runtime_epoch: AtomicU64,
    invalidation_epoch: AtomicU64,
    shutting_down: AtomicBool,
    active: Mutex<ActiveDiscoveryState>,
    active_changed: Condvar,
}

impl CodexDiscoveryManager {
    pub(crate) fn new(registration: CodexBindingRegistration) -> Arc<Self> {
        Arc::new(Self {
            registration,
            admission_occupied: Arc::new(AtomicBool::new(false)),
            cache: Mutex::new(None),
            next_runtime_epoch: AtomicU64::new(1),
            invalidation_epoch: AtomicU64::new(1),
            shutting_down: AtomicBool::new(false),
            active: Mutex::new(ActiveDiscoveryState::default()),
            active_changed: Condvar::new(),
        })
    }

    pub(crate) fn prepare(
        &self,
        request: GetCodexDiscoveryRequest,
        project_root: CodexProjectRootAuthority,
        deadline: CodexDiscoveryDeadline,
    ) -> Result<PreparedCodexDiscovery, CodexDiscoveryRunError> {
        deadline.ensure_operation()?;
        validate_scope(&request, &project_root)?;
        let admission = self.acquire_admission()?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(CodexDiscoveryRunError::Unavailable);
        }
        Ok(PreparedCodexDiscovery {
            request,
            project_root,
            invalidation_epoch: self.invalidation_epoch.load(Ordering::Acquire),
            deadline,
            _admission: admission,
        })
    }

    pub(crate) fn discover(
        &self,
        prepared: PreparedCodexDiscovery,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        let active = self.begin_active()?;
        let result = self.discover_active(prepared, &active);
        let settlement = if result == Err(CodexDiscoveryRunError::Cleanup) {
            Err(CodexDiscoveryRunError::Cleanup)
        } else {
            Ok(())
        };
        active.settle(settlement);
        result
    }

    fn discover_active(
        &self,
        prepared: PreparedCodexDiscovery,
        active: &ActiveDiscoveryGuard<'_>,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        if self.invalidation_epoch.load(Ordering::Acquire) != prepared.invalidation_epoch {
            return Err(CodexDiscoveryRunError::Drift);
        }
        let PreparedCodexDiscovery {
            request,
            project_root,
            invalidation_epoch,
            deadline,
            _admission,
        } = prepared;
        deadline.ensure_operation()?;
        let binding = self.registration.refresh().map_err(|error| {
            self.clear_cache();
            match error {
                crate::acp::bindings::codex::CodexAdmissionUnavailable::Drift => {
                    CodexDiscoveryRunError::Drift
                }
                crate::acp::bindings::codex::CodexAdmissionUnavailable::Platform => {
                    CodexDiscoveryRunError::Unavailable
                }
                crate::acp::bindings::codex::CodexAdmissionUnavailable::IndependentAdmission => {
                    CodexDiscoveryRunError::BindingUnavailable
                }
            }
        })?;
        if request.binding_id.as_str() != binding.binding_id() {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        project_root.revalidate().map_err(|_| {
            self.clear_cache();
            CodexDiscoveryRunError::Drift
        })?;
        let scope = CacheScope {
            project_id: request.project_id.clone(),
            project_generation: request.project_generation,
            root_id: project_root.root_id().clone(),
            root_identity: project_root.identity_bytes(),
            binding_digest: binding.binding_digest(),
        };
        if let Some(response) = self.cached(&scope)? {
            return Ok(response);
        }

        let runtime_epoch = self.allocate_runtime_epoch()?;
        let discovery =
            discover_admitted(binding, project_root, runtime_epoch, deadline, |control| {
                active.register_control(control)
            })
            .inspect_err(|error| {
                self.clear_cache();
                if *error == CodexDiscoveryRunError::Drift {
                    self.registration.invalidate_drift();
                }
            })?;
        deadline.ensure_total()?;
        discovery.revalidate().inspect_err(|_| self.clear_cache())?;
        let response = response_from(
            request.binding_id,
            scope.clone(),
            discovery.runtime_epoch(),
            &discovery.content,
        );
        deadline.ensure_total()?;
        self.publish(
            invalidation_epoch,
            CacheKey {
                scope,
                runtime_epoch,
            },
            response.clone(),
        )?;
        Ok(response)
    }

    pub(crate) fn invalidate(&self) {
        let _ = self.invalidate_active().1;
        self.clear_cache();
    }

    pub(crate) fn invalidate_and_wait(
        &self,
        timeout: Duration,
    ) -> Result<(), CodexDiscoveryRunError> {
        let (was_running, cancellation) = self.invalidate_active();
        self.clear_cache();
        let convergence = self.wait_for_idle(timeout, was_running);
        cancellation.and(convergence)
    }

    pub(crate) fn shutdown(&self, timeout: Duration) -> Result<(), CodexDiscoveryRunError> {
        self.shutting_down.store(true, Ordering::Release);
        let (was_running, cancellation) = self.invalidate_active();
        self.clear_cache();
        let convergence = self.wait_for_idle(timeout, was_running);
        cancellation.and(convergence)
    }

    fn wait_for_idle(
        &self,
        timeout: Duration,
        require_settlement: bool,
    ) -> Result<(), CodexDiscoveryRunError> {
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

    fn allocate_runtime_epoch(&self) -> Result<RuntimeEpoch, CodexDiscoveryRunError> {
        let epoch = self.next_runtime_epoch.fetch_add(1, Ordering::AcqRel);
        RuntimeEpoch::new(epoch).map_err(|_| CodexDiscoveryRunError::Transport)
    }

    fn cached(
        &self,
        scope: &CacheScope,
    ) -> Result<Option<CodexDiscoveryResponse>, CodexDiscoveryRunError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        Ok(cache.as_ref().and_then(|entry| entry.response_for(scope)))
    }

    fn publish(
        &self,
        expected_invalidation_epoch: u64,
        key: CacheKey,
        response: CodexDiscoveryResponse,
    ) -> Result<(), CodexDiscoveryRunError> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        if self.invalidation_epoch.load(Ordering::Acquire) != expected_invalidation_epoch {
            return Err(CodexDiscoveryRunError::Drift);
        }
        *cache = Some(CachedDiscovery { key, response });
        Ok(())
    }

    fn clear_cache(&self) {
        match self.cache.lock() {
            Ok(mut cache) => *cache = None,
            Err(poisoned) => *poisoned.into_inner() = None,
        }
    }

    fn acquire_admission(&self) -> Result<DiscoveryAdmissionLease, CodexDiscoveryRunError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(CodexDiscoveryRunError::Unavailable);
        }
        self.admission_occupied
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| CodexDiscoveryRunError::Busy)?;
        Ok(DiscoveryAdmissionLease {
            occupied: Arc::clone(&self.admission_occupied),
        })
    }

    fn begin_active(&self) -> Result<ActiveDiscoveryGuard<'_>, CodexDiscoveryRunError> {
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
        active.cancel_requested = false;
        active.cancellation = None;
        active.settlement = None;
        Ok(ActiveDiscoveryGuard {
            manager: self,
            settled: false,
        })
    }

    fn invalidate_active(&self) -> (bool, Result<(), CodexDiscoveryRunError>) {
        self.invalidate_active_with(|| {})
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

    fn finish_active(&self, settlement: Result<(), CodexDiscoveryRunError>) {
        match self.active.lock() {
            Ok(mut active) => {
                active.running = false;
                active.cancel_requested = false;
                active.cancellation = None;
                active.settlement = Some(settlement);
                self.active_changed.notify_all();
            }
            Err(poisoned) => {
                let mut active = poisoned.into_inner();
                active.running = false;
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
    use crate::acp::{
        bindings::codex::{
            CodexDiscoveryProtocol, CodexProjectRootAuthority, DiscoveryTransport,
            DiscoveryZeroProcess, RevalidationCheckpoint, parse_packet_metadata,
            run_discovery_protocol, startup_codex_binding_registration,
        },
        contract::{ProjectGeneration, RequiredText, RuntimeEpoch, SageProjectId, SageRootId},
        intent::{BindingId, ModelId},
        ipc::{CodexDiscoveryAccount, CodexDiscoveryModel},
        transport::{Frame, FrameWriteOutcome, OutboundFrame},
    };
    use crate::project_registry::ProjectRootAuthority;
    use std::{
        sync::{Condvar, mpsc},
        thread,
        time::Duration,
    };

    struct TestCancellation {
        requested: Mutex<bool>,
        changed: Condvar,
        fail_dispatch: bool,
    }

    struct BlockingTransport {
        cancellation: Arc<TestCancellation>,
        cleanup: Result<u64, CodexDiscoveryRunError>,
        closed: Arc<AtomicBool>,
        reader_joined: Arc<AtomicBool>,
    }

    impl DiscoveryTransport for BlockingTransport {
        fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
            FrameWriteOutcome::Complete
        }

        fn read_frame(&mut self, _: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError> {
            self.cancellation.wait_requested();
            Err(CodexDiscoveryRunError::Transport)
        }

        fn revalidate(&self, _: RevalidationCheckpoint) -> Result<(), CodexDiscoveryRunError> {
            Ok(())
        }

        fn close_stdin(&mut self) {
            self.closed.store(true, Ordering::Release);
        }

        fn terminate_zero(
            &mut self,
            _: bool,
            _: CodexDiscoveryDeadline,
        ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
            self.reader_joined.store(true, Ordering::Release);
            self.cleanup.map(DiscoveryZeroProcess::fixture)
        }
    }

    impl TestCancellation {
        fn new(fail_dispatch: bool) -> Arc<Self> {
            Arc::new(Self {
                requested: Mutex::new(false),
                changed: Condvar::new(),
                fail_dispatch,
            })
        }

        fn wait_requested(&self) {
            let requested = self.requested.lock().unwrap();
            let (requested, wait) = self
                .changed
                .wait_timeout_while(requested, Duration::from_millis(500), |value| !*value)
                .unwrap();
            assert!(*requested && !wait.timed_out());
        }
    }

    impl DiscoveryCancellation for TestCancellation {
        fn request_parent_shutdown(&self) -> Result<(), CodexDiscoveryRunError> {
            *self.requested.lock().unwrap() = true;
            self.changed.notify_all();
            if self.fail_dispatch {
                Err(CodexDiscoveryRunError::Cleanup)
            } else {
                Ok(())
            }
        }
    }

    fn scope() -> CacheScope {
        CacheScope {
            project_id: SageProjectId::parse("project-cache").unwrap(),
            project_generation: ProjectGeneration::new(7).unwrap(),
            root_id: SageRootId::parse("root-cache").unwrap(),
            root_identity: [3; 24],
            binding_digest: [5; 32],
        }
    }

    fn response(scope: &CacheScope, runtime_epoch: RuntimeEpoch) -> CodexDiscoveryResponse {
        let model_id = ModelId::new("modelId", "gpt-cache").unwrap();
        CodexDiscoveryResponse {
            project_id: scope.project_id.clone(),
            project_generation: scope.project_generation,
            root_id: scope.root_id.clone(),
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
            runtime_epoch,
            account: CodexDiscoveryAccount::ManagedChatgpt,
            models: vec![CodexDiscoveryModel {
                id: model_id.clone(),
                name: RequiredText::new("modelName", "GPT Cache").unwrap(),
                is_default: true,
            }],
            default_model_id: Some(model_id),
        }
    }

    #[test]
    fn cache_requires_the_exact_project_root_binding_and_runtime_scope() {
        let scope = scope();
        let runtime_epoch = RuntimeEpoch::new(11).unwrap();
        let entry = CachedDiscovery {
            key: CacheKey {
                scope: scope.clone(),
                runtime_epoch,
            },
            response: response(&scope, runtime_epoch),
        };
        assert!(entry.response_for(&scope).is_some());

        let mut changed = scope.clone();
        changed.project_generation = ProjectGeneration::new(8).unwrap();
        assert!(entry.response_for(&changed).is_none());
        changed = scope.clone();
        changed.project_id = SageProjectId::parse("project-other").unwrap();
        assert!(entry.response_for(&changed).is_none());
        changed = scope.clone();
        changed.root_id = SageRootId::parse("root-other").unwrap();
        assert!(entry.response_for(&changed).is_none());
        changed = scope.clone();
        changed.root_identity = [4; 24];
        assert!(entry.response_for(&changed).is_none());
        changed = scope.clone();
        changed.binding_digest = [6; 32];
        assert!(entry.response_for(&changed).is_none());

        let mismatched_runtime = CachedDiscovery {
            key: CacheKey {
                scope: scope.clone(),
                runtime_epoch,
            },
            response: response(&scope, RuntimeEpoch::new(12).unwrap()),
        };
        assert!(mismatched_runtime.response_for(&scope).is_none());
    }

    #[test]
    fn explicit_invalidation_drops_the_entire_memory_only_snapshot() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let scope = scope();
        let runtime_epoch = RuntimeEpoch::new(13).unwrap();
        manager
            .publish(
                manager.invalidation_epoch.load(Ordering::Acquire),
                CacheKey {
                    scope: scope.clone(),
                    runtime_epoch,
                },
                response(&scope, runtime_epoch),
            )
            .unwrap();
        assert!(manager.cached(&scope).unwrap().is_some());

        manager.invalidate();

        assert!(manager.cached(&scope).unwrap().is_none());
    }

    #[test]
    fn invalidation_between_discovery_and_publish_cannot_reinsert_a_stale_snapshot() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let scope = scope();
        let runtime_epoch = RuntimeEpoch::new(14).unwrap();
        let stale_epoch = manager.invalidation_epoch.load(Ordering::Acquire);

        manager.invalidate();
        assert_eq!(
            manager
                .publish(
                    stale_epoch,
                    CacheKey {
                        scope: scope.clone(),
                        runtime_epoch,
                    },
                    response(&scope, runtime_epoch),
                )
                .unwrap_err(),
            CodexDiscoveryRunError::Drift
        );
        assert!(manager.cached(&scope).unwrap().is_none());
    }

    #[test]
    fn concurrent_discovery_is_rejected_without_waiting() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let held = manager.acquire_admission().unwrap();
        let concurrent = Arc::clone(&manager);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let result = concurrent.acquire_admission().map(drop);
            sender.send(result).unwrap();
        });

        assert_eq!(
            receiver.recv_timeout(Duration::from_millis(100)),
            Ok(Err(CodexDiscoveryRunError::Busy)),
        );
        drop(held);
        worker.join().unwrap();
        let transferable = manager.acquire_admission().unwrap();
        thread::spawn(move || drop(transferable)).join().unwrap();
        assert!(manager.acquire_admission().is_ok());
    }

    #[test]
    fn project_switch_after_preparation_rejects_discovery_before_launch() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let request = GetCodexDiscoveryRequest {
            project_id: SageProjectId::parse("project-prepared").unwrap(),
            project_generation: ProjectGeneration::new(19).unwrap(),
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
        };
        let root = CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                request.project_id.clone(),
                request.project_generation,
                SageRootId::parse("root-prepared").unwrap(),
                &std::env::current_dir().unwrap(),
            )
            .unwrap(),
        );
        let prepared = manager
            .prepare(request, root, CodexDiscoveryDeadline::new().unwrap())
            .unwrap();

        manager.invalidate();

        assert_eq!(
            manager.discover(prepared).unwrap_err(),
            CodexDiscoveryRunError::Drift
        );
    }

    #[test]
    fn shutdown_waits_for_the_active_discovery_to_finish() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let active = manager.begin_active().unwrap();
        let shutting_down = Arc::clone(&manager);
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            sender
                .send(shutting_down.shutdown(Duration::from_millis(500)))
                .unwrap();
        });

        assert_eq!(
            receiver.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        active.settle(Ok(()));
        assert_eq!(
            receiver.recv_timeout(Duration::from_millis(500)),
            Ok(Ok(()))
        );
        worker.join().unwrap();
    }

    #[test]
    fn shutdown_invalidates_cached_discovery_and_rejects_new_activity() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let scope = scope();
        let runtime_epoch = RuntimeEpoch::new(21).unwrap();
        manager
            .publish(
                manager.invalidation_epoch.load(Ordering::Acquire),
                CacheKey {
                    scope: scope.clone(),
                    runtime_epoch,
                },
                response(&scope, runtime_epoch),
            )
            .unwrap();

        manager.shutdown(Duration::from_millis(100)).unwrap();

        assert!(manager.cached(&scope).unwrap().is_none());
        assert!(matches!(
            manager.begin_active(),
            Err(CodexDiscoveryRunError::Unavailable)
        ));
    }

    #[test]
    fn project_invalidation_dispatches_cancellation_and_awaits_operation_zero() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let cancellation = TestCancellation::new(false);
        let worker_manager = Arc::clone(&manager);
        let worker_cancellation = Arc::clone(&cancellation);
        let closed = Arc::new(AtomicBool::new(false));
        let reader_joined = Arc::new(AtomicBool::new(false));
        let worker_closed = Arc::clone(&closed);
        let worker_joined = Arc::clone(&reader_joined);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let active = worker_manager.begin_active().unwrap();
            let cancellation_authority: Arc<dyn DiscoveryCancellation> =
                worker_cancellation.clone();
            active
                .register_cancellation(cancellation_authority)
                .unwrap();
            let mut protocol = discovery_protocol();
            let mut transport = BlockingTransport {
                cancellation: worker_cancellation,
                cleanup: Ok(71),
                closed: worker_closed,
                reader_joined: worker_joined,
            };
            ready_tx.send(()).unwrap();
            let result = run_discovery_protocol(
                &mut protocol,
                &mut transport,
                71,
                CodexDiscoveryDeadline::new().unwrap(),
            );
            active.settle(if result == Err(CodexDiscoveryRunError::Cleanup) {
                Err(CodexDiscoveryRunError::Cleanup)
            } else {
                Ok(())
            });
            result_tx.send(result).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_millis(500)).unwrap();

        manager
            .invalidate_and_wait(Duration::from_millis(500))
            .unwrap();

        assert_eq!(
            result_rx.recv_timeout(Duration::from_millis(500)),
            Ok(Err(CodexDiscoveryRunError::Transport))
        );
        assert!(closed.load(Ordering::Acquire));
        assert!(reader_joined.load(Ordering::Acquire));
        worker.join().unwrap();
    }

    #[test]
    fn project_invalidation_surfaces_failed_zero_process_proof() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let cancellation = TestCancellation::new(false);
        let worker_manager = Arc::clone(&manager);
        let worker_cancellation = Arc::clone(&cancellation);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let active = worker_manager.begin_active().unwrap();
            let cancellation_authority: Arc<dyn DiscoveryCancellation> =
                worker_cancellation.clone();
            active
                .register_cancellation(cancellation_authority)
                .unwrap();
            let mut protocol = discovery_protocol();
            let mut transport = BlockingTransport {
                cancellation: worker_cancellation,
                cleanup: Err(CodexDiscoveryRunError::Cleanup),
                closed: Arc::new(AtomicBool::new(false)),
                reader_joined: Arc::new(AtomicBool::new(false)),
            };
            ready_tx.send(()).unwrap();
            let result = run_discovery_protocol(
                &mut protocol,
                &mut transport,
                71,
                CodexDiscoveryDeadline::new().unwrap(),
            );
            active.settle(Err(CodexDiscoveryRunError::Cleanup));
            result
        });
        ready_rx.recv_timeout(Duration::from_millis(500)).unwrap();

        assert_eq!(
            manager.invalidate_and_wait(Duration::from_millis(500)),
            Err(CodexDiscoveryRunError::Cleanup)
        );
        assert_eq!(worker.join().unwrap(), Err(CodexDiscoveryRunError::Cleanup));
    }

    #[test]
    fn invalidation_epoch_and_active_settlement_capture_are_linearized() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let active = manager.begin_active().unwrap();
        let cancellation = TestCancellation::new(false);
        let cancellation_authority: Arc<dyn DiscoveryCancellation> = cancellation.clone();
        active
            .register_cancellation(cancellation_authority)
            .unwrap();
        let epoch = manager.invalidation_epoch.load(Ordering::Acquire);
        let held_state = manager.active.lock().unwrap();
        let invalidating = Arc::clone(&manager);
        let (attempted_tx, attempted_rx) = mpsc::sync_channel(1);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let (was_running, cancellation) = invalidating.invalidate_active_with(|| {
                attempted_tx.send(()).unwrap();
            });
            let convergence = invalidating.wait_for_idle(Duration::from_millis(500), was_running);
            result_tx.send(cancellation.and(convergence)).unwrap();
        });
        attempted_rx
            .recv_timeout(Duration::from_millis(500))
            .unwrap();

        assert_eq!(manager.invalidation_epoch.load(Ordering::Acquire), epoch);
        assert_eq!(
            result_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        drop(held_state);
        cancellation.wait_requested();
        active.settle(Err(CodexDiscoveryRunError::Cleanup));

        assert_eq!(
            result_rx.recv_timeout(Duration::from_millis(500)),
            Ok(Err(CodexDiscoveryRunError::Cleanup))
        );
        assert_eq!(
            manager.invalidation_epoch.load(Ordering::Acquire),
            epoch + 1
        );
        worker.join().unwrap();
    }

    #[test]
    fn project_invalidation_surfaces_cancellation_dispatch_failure_after_convergence() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let cancellation = TestCancellation::new(true);
        let worker_manager = Arc::clone(&manager);
        let worker_cancellation = Arc::clone(&cancellation);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let active = worker_manager.begin_active().unwrap();
            active.register_cancellation(worker_cancellation).unwrap();
            ready_tx.send(()).unwrap();
            cancellation.wait_requested();
            active.settle(Ok(()));
        });
        ready_rx.recv_timeout(Duration::from_millis(500)).unwrap();

        assert_eq!(
            manager
                .invalidate_and_wait(Duration::from_millis(500))
                .unwrap_err(),
            CodexDiscoveryRunError::Cleanup
        );
        worker.join().unwrap();
    }

    fn discovery_protocol() -> CodexDiscoveryProtocol {
        let packet = parse_packet_metadata(include_bytes!(
            "../../../testdata/acp/codex/candidate-windows.json"
        ))
        .unwrap();
        CodexDiscoveryProtocol::new(&packet, [9; 32], RuntimeEpoch::new(71).unwrap())
    }
}
