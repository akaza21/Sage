use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::acp::{
    bindings::codex::{
        CodexBindingRegistration, CodexDiscoveryDeadline, CodexDiscoveryRunError,
        CodexProjectRootAuthority, discover_admitted,
    },
    ipc::{CodexDiscoveryResponse, GetCodexDiscoveryRequest},
};

use super::codex_operation::{ActiveCodexOperation, CodexOperationGate, PreparedCodexOperation};

mod cache;
use cache::{CacheKey, CacheScope, CachedDiscovery, response_from, validate_scope};

/// Opaque authority prepared while the active-Project transition is held.
/// Discovery must reject it if any later transition invalidates that moment.
pub(crate) struct PreparedCodexDiscovery {
    request: GetCodexDiscoveryRequest,
    project_root: CodexProjectRootAuthority,
    operation: PreparedCodexOperation,
    deadline: CodexDiscoveryDeadline,
}

pub(crate) struct CodexDiscoveryManager {
    registration: CodexBindingRegistration,
    gate: Arc<CodexOperationGate>,
    cache: Mutex<Option<CachedDiscovery>>,
}

impl CodexDiscoveryManager {
    #[cfg(test)]
    pub(crate) fn new(registration: CodexBindingRegistration) -> Arc<Self> {
        Self::with_gate(registration, CodexOperationGate::new())
    }

    pub(super) fn with_gate(
        registration: CodexBindingRegistration,
        gate: Arc<CodexOperationGate>,
    ) -> Arc<Self> {
        Arc::new(Self {
            registration,
            gate,
            cache: Mutex::new(None),
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
        let operation = self.gate.prepare()?;
        Ok(PreparedCodexDiscovery {
            request,
            project_root,
            operation,
            deadline,
        })
    }

    pub(crate) fn discover(
        &self,
        prepared: PreparedCodexDiscovery,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        let active = self.gate.begin(&prepared.operation)?;
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
        active: &ActiveCodexOperation,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        let PreparedCodexDiscovery {
            request,
            project_root,
            operation,
            deadline,
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

        let runtime_epoch = self.gate.allocate_runtime_epoch()?;
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
            CodexOperationGate::prepared_epoch(&operation),
            CacheKey {
                scope,
                runtime_epoch,
            },
            response.clone(),
        )?;
        Ok(response)
    }

    pub(crate) fn invalidate(&self) {
        self.gate.invalidate();
        self.clear_cache();
    }

    pub(crate) fn forget_snapshot(&self) {
        self.clear_cache();
    }

    pub(crate) fn invalidate_and_wait(
        &self,
        timeout: Duration,
    ) -> Result<(), CodexDiscoveryRunError> {
        self.clear_cache();
        self.gate.invalidate_and_wait(timeout)
    }

    pub(crate) fn shutdown(&self, timeout: Duration) -> Result<(), CodexDiscoveryRunError> {
        self.clear_cache();
        self.gate.shutdown(timeout)
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
        if !self.gate.is_current(expected_invalidation_epoch) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::service::codex_operation::CodexOperationCancellation;
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
        sync::{
            Condvar,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
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

    impl CodexOperationCancellation for TestCancellation {
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
                manager.gate.current_epoch(),
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
    fn effect_unknown_recovery_can_forget_only_the_cached_snapshot() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let scope = scope();
        let runtime_epoch = RuntimeEpoch::new(113).unwrap();
        manager
            .publish(
                manager.gate.current_epoch(),
                CacheKey {
                    scope: scope.clone(),
                    runtime_epoch,
                },
                response(&scope, runtime_epoch),
            )
            .unwrap();
        let operation_epoch = manager.gate.current_epoch();

        manager.forget_snapshot();

        assert!(manager.cached(&scope).unwrap().is_none());
        assert_eq!(manager.gate.current_epoch(), operation_epoch);
    }

    #[test]
    fn invalidation_between_discovery_and_publish_cannot_reinsert_a_stale_snapshot() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let scope = scope();
        let runtime_epoch = RuntimeEpoch::new(14).unwrap();
        let stale_epoch = manager.gate.current_epoch();

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
        let prepared = manager.gate.prepare().unwrap();
        let active = manager.gate.begin(&prepared).unwrap();
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
                manager.gate.current_epoch(),
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
            manager.gate.prepare(),
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
            let prepared = worker_manager.gate.prepare().unwrap();
            let active = worker_manager.gate.begin(&prepared).unwrap();
            let cancellation_authority: Arc<dyn CodexOperationCancellation> =
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
            let prepared = worker_manager.gate.prepare().unwrap();
            let active = worker_manager.gate.begin(&prepared).unwrap();
            let cancellation_authority: Arc<dyn CodexOperationCancellation> =
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
    fn project_invalidation_surfaces_cancellation_dispatch_failure_after_convergence() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let cancellation = TestCancellation::new(true);
        let worker_manager = Arc::clone(&manager);
        let worker_cancellation = Arc::clone(&cancellation);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let prepared = worker_manager.gate.prepare().unwrap();
            let active = worker_manager.gate.begin(&prepared).unwrap();
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
