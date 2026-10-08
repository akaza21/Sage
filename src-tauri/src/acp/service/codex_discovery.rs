use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::acp::{
    bindings::codex::{
        AdmittedCodexBinding, CodexBindingRegistration, CodexDiscoveryDeadline,
        CodexDiscoveryRunError, CodexProjectRootAuthority, discover_admitted,
    },
    ipc::{CodexDiscoveryResponse, GetCodexDiscoveryRequest},
};

use super::codex_operation::{ActiveCodexOperation, CodexOperationGate, PreparedCodexOperation};

mod cache;
use cache::{CacheKey, CacheScope, DiscoveryCache, response_from, validate_scope};
mod flight;
use flight::{DiscoveryFlight, DiscoveryFlightKey};

/// Opaque authority prepared while the active-Project transition is held.
/// Discovery must reject it if any later transition invalidates that moment.
pub(crate) struct PreparedCodexDiscovery {
    request: GetCodexDiscoveryRequest,
    project_root: Option<CodexProjectRootAuthority>,
    operation: Option<PreparedCodexOperation>,
    deadline: CodexDiscoveryDeadline,
    invalidation_epoch: u64,
    flight: Arc<DiscoveryFlight>,
    owns_flight: bool,
    completed: bool,
}

impl Drop for PreparedCodexDiscovery {
    fn drop(&mut self) {
        if self.owns_flight && !self.completed {
            self.flight.complete(Err(CodexDiscoveryRunError::Cleanup));
        }
    }
}

pub(crate) struct CodexDiscoveryManager {
    registration: CodexBindingRegistration,
    gate: Arc<CodexOperationGate>,
    cache: DiscoveryCache,
    flight: Mutex<Option<Arc<DiscoveryFlight>>>,
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
            cache: DiscoveryCache::new(),
            flight: Mutex::new(None),
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
        let invalidation_epoch = self.gate.current_epoch();
        let key = DiscoveryFlightKey {
            project_id: request.project_id.clone(),
            project_generation: request.project_generation,
            root_id: project_root.root_id().clone(),
            root_identity: project_root.identity_bytes(),
            binding_id: request.binding_id.clone(),
        };
        let mut flight = self
            .flight
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        if let Some(existing) = flight
            .as_ref()
            .filter(|existing| existing.matches_active(&key))
        {
            return Ok(PreparedCodexDiscovery {
                request,
                project_root: Some(project_root),
                operation: None,
                deadline,
                invalidation_epoch,
                flight: Arc::clone(existing),
                owns_flight: false,
                completed: false,
            });
        }
        let operation = self.gate.prepare()?;
        let active_flight = DiscoveryFlight::new(key);
        *flight = Some(Arc::clone(&active_flight));
        Ok(PreparedCodexDiscovery {
            request,
            project_root: Some(project_root),
            operation: Some(operation),
            deadline,
            invalidation_epoch,
            flight: active_flight,
            owns_flight: true,
            completed: false,
        })
    }

    pub(crate) fn discover(
        &self,
        mut prepared: PreparedCodexDiscovery,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        if !prepared.owns_flight {
            let joined = prepared
                .flight
                .wait_until(prepared.deadline.cleanup_end())?;
            return self.revalidate_joined(&prepared, &joined);
        }
        let flight = Arc::clone(&prepared.flight);
        let result = self.discover_owned(&mut prepared);
        flight.complete(result.clone());
        prepared.completed = true;
        self.clear_flight(&flight);
        result
    }

    fn discover_owned(
        &self,
        prepared: &mut PreparedCodexDiscovery,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        let operation = prepared
            .operation
            .as_ref()
            .ok_or(CodexDiscoveryRunError::Cleanup)?;
        self.gate.validate_admission(operation)?;
        let (binding, scope) = self.resolve_scope(prepared)?;
        if let Some(response) = self.cache.response_for(&scope)? {
            self.gate.validate_admission(operation)?;
            return Ok(response);
        }
        let operation_epoch = CodexOperationGate::prepared_epoch(operation);
        let active = self.gate.begin(operation)?;
        let result = self.probe_admitted(prepared, &active, binding, scope, operation_epoch);
        let settlement = if result == Err(CodexDiscoveryRunError::Cleanup) {
            Err(CodexDiscoveryRunError::Cleanup)
        } else {
            Ok(())
        };
        active.settle(settlement);
        result
    }

    fn revalidate_joined(
        &self,
        prepared: &PreparedCodexDiscovery,
        joined: &CodexDiscoveryResponse,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        prepared.deadline.ensure_total()?;
        self.gate.validate_epoch(prepared.invalidation_epoch)?;
        let (_, scope) = self.scope_for(prepared)?;
        let response = self
            .cache
            .response_for(&scope)?
            .filter(|cached| {
                cached.binding_id == joined.binding_id
                    && cached.runtime_epoch == joined.runtime_epoch
            })
            .ok_or(CodexDiscoveryRunError::Drift)?;
        prepared.deadline.ensure_total()?;
        self.gate.validate_epoch(prepared.invalidation_epoch)?;
        Ok(response)
    }

    fn resolve_scope(
        &self,
        prepared: &PreparedCodexDiscovery,
    ) -> Result<(Arc<AdmittedCodexBinding>, CacheScope), CodexDiscoveryRunError> {
        prepared.deadline.ensure_operation()?;
        self.scope_for(prepared)
    }

    fn scope_for(
        &self,
        prepared: &PreparedCodexDiscovery,
    ) -> Result<(Arc<AdmittedCodexBinding>, CacheScope), CodexDiscoveryRunError> {
        let binding = self.registration.refresh().map_err(|error| {
            self.cache.clear();
            match error {
                crate::acp::bindings::codex::CodexAdmissionUnavailable::Drift => {
                    CodexDiscoveryRunError::Drift
                }
                crate::acp::bindings::codex::CodexAdmissionUnavailable::Projection(_) => {
                    CodexDiscoveryRunError::CapabilityProjectionUnavailable
                }
                crate::acp::bindings::codex::CodexAdmissionUnavailable::Platform
                | crate::acp::bindings::codex::CodexAdmissionUnavailable::ObservationOnly => {
                    CodexDiscoveryRunError::Unavailable
                }
                crate::acp::bindings::codex::CodexAdmissionUnavailable::IndependentAdmission => {
                    CodexDiscoveryRunError::BindingUnavailable
                }
            }
        })?;
        if prepared.request.binding_id.as_str() != binding.binding_id() {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        let project_root = prepared
            .project_root
            .as_ref()
            .ok_or(CodexDiscoveryRunError::Cleanup)?;
        project_root.revalidate().map_err(|_| {
            self.cache.clear();
            CodexDiscoveryRunError::Drift
        })?;
        let binding_digest = binding.binding_digest();
        let scope = CacheScope {
            project_id: prepared.request.project_id.clone(),
            project_generation: prepared.request.project_generation,
            root_id: project_root.root_id().clone(),
            root_identity: project_root.identity_bytes(),
            binding_digest,
        };
        Ok((binding, scope))
    }

    fn probe_admitted(
        &self,
        prepared: &mut PreparedCodexDiscovery,
        active: &ActiveCodexOperation,
        binding: Arc<AdmittedCodexBinding>,
        scope: CacheScope,
        operation_epoch: u64,
    ) -> Result<CodexDiscoveryResponse, CodexDiscoveryRunError> {
        let runtime_epoch = self.gate.allocate_runtime_epoch()?;
        let discovery = discover_admitted(
            binding,
            prepared
                .project_root
                .take()
                .ok_or(CodexDiscoveryRunError::Cleanup)?,
            runtime_epoch,
            prepared.deadline,
            |control| active.register_control(control),
        )
        .inspect_err(|error| {
            self.cache.clear();
            if *error == CodexDiscoveryRunError::Drift {
                self.registration.invalidate_drift();
            }
        })?;
        prepared.deadline.ensure_total()?;
        discovery.revalidate().inspect_err(|_| self.cache.clear())?;
        let response = response_from(
            prepared.request.binding_id.clone(),
            scope.clone(),
            discovery.runtime_epoch(),
            &discovery.content,
        );
        prepared.deadline.ensure_total()?;
        self.publish(
            operation_epoch,
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
        self.cache.clear();
    }

    pub(crate) fn forget_snapshot(&self) {
        self.cache.clear();
    }

    pub(super) fn validate_session_model(
        &self,
        project_root: &CodexProjectRootAuthority,
        binding_id: &crate::acp::intent::BindingId,
        binding_digest: [u8; 32],
        model_id: Option<&crate::acp::intent::ModelId>,
    ) -> Result<(), CodexDiscoveryRunError> {
        project_root
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        self.cache
            .validate_session_model(project_root, binding_id, binding_digest, model_id)
    }

    #[cfg(test)]
    pub(crate) fn invalidate_and_wait(
        &self,
        timeout: Duration,
    ) -> Result<(), CodexDiscoveryRunError> {
        self.cache.clear();
        self.gate.invalidate_and_wait(timeout)
    }

    /// Project replacement passes its original absolute deadline through to
    /// cancellation dispatch and gate settlement.
    pub(crate) fn project_switched_until(
        &self,
        deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError> {
        self.cache.clear();
        self.gate.invalidate_ephemeral_and_wait_until(deadline)
    }

    pub(crate) fn shutdown(&self, timeout: Duration) -> Result<(), CodexDiscoveryRunError> {
        self.cache.clear();
        self.gate.shutdown(timeout)
    }

    fn publish(
        &self,
        expected_invalidation_epoch: u64,
        key: CacheKey,
        response: CodexDiscoveryResponse,
    ) -> Result<(), CodexDiscoveryRunError> {
        self.cache.publish_if_current(key, response, || {
            self.gate.is_current(expected_invalidation_epoch)
        })
    }

    fn clear_flight(&self, completed: &Arc<DiscoveryFlight>) {
        let mut flight = self
            .flight
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if flight
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, completed))
        {
            *flight = None;
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn seed_session_validation_for_test(
        &self,
        project_root: &CodexProjectRootAuthority,
        binding_digest: [u8; 32],
        model: Option<crate::acp::intent::ModelId>,
    ) {
        self.seed_session_validation_for_binding_for_test(
            project_root,
            binding_digest,
            crate::acp::bindings::codex::CODEX_BINDING_ID,
            model,
        );
    }

    #[cfg(all(test, windows))]
    pub(crate) fn seed_session_validation_for_binding_for_test(
        &self,
        project_root: &CodexProjectRootAuthority,
        binding_digest: [u8; 32],
        binding_id: &str,
        model: Option<crate::acp::intent::ModelId>,
    ) {
        self.cache
            .seed_session_validation(project_root, binding_digest, binding_id, model);
    }
}

#[cfg(test)]
mod tests {
    use super::cache::CachedDiscovery;
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
    use std::fs;
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
        fn request_parent_shutdown_until(
            &self,
            _deadline: std::time::Instant,
        ) -> Result<(), CodexDiscoveryRunError> {
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
    fn cache_reuses_a_catalog_across_a_same_project_layout_transition() {
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
        let reused = entry.response_for(&changed).unwrap();
        assert_eq!(reused.project_generation, changed.project_generation);
        assert_eq!(reused.runtime_epoch, runtime_epoch);
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
        assert!(manager.cache.response_for(&scope).unwrap().is_some());

        manager.invalidate();

        assert!(manager.cache.response_for(&scope).unwrap().is_none());
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

        assert!(manager.cache.response_for(&scope).unwrap().is_none());
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
        assert!(manager.cache.response_for(&scope).unwrap().is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn session_model_validation_reports_root_drift_before_inspecting_the_cache() {
        let root =
            std::env::temp_dir().join(format!("sage-discovery-drift-{}", uuid::Uuid::new_v4()));
        let moved = root.with_extension("moved");
        fs::create_dir_all(&root).unwrap();
        let project_id = SageProjectId::parse("project-session-drift").unwrap();
        let authority = CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                project_id,
                ProjectGeneration::new(31).unwrap(),
                SageRootId::parse("root-session-drift").unwrap(),
                &root,
            )
            .unwrap(),
        );
        fs::rename(&root, &moved).unwrap();
        fs::create_dir_all(&root).unwrap();
        assert!(authority.revalidate().is_err());

        let binding_id = BindingId::new("bindingId", "codex-app-server").unwrap();
        let empty = CodexDiscoveryManager::new(startup_codex_binding_registration());
        assert_eq!(
            empty
                .validate_session_model(&authority, &binding_id, [7; 32], None)
                .unwrap_err(),
            CodexDiscoveryRunError::Drift
        );

        let mismatched = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let cache_scope = scope();
        let runtime_epoch = RuntimeEpoch::new(19).unwrap();
        mismatched
            .publish(
                mismatched.gate.current_epoch(),
                CacheKey {
                    scope: cache_scope.clone(),
                    runtime_epoch,
                },
                response(&cache_scope, runtime_epoch),
            )
            .unwrap();
        assert_eq!(
            mismatched
                .validate_session_model(&authority, &binding_id, [7; 32], None)
                .unwrap_err(),
            CodexDiscoveryRunError::Drift
        );

        drop(authority);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(moved).unwrap();
    }

    #[test]
    fn session_model_validation_rejects_a_stale_root_before_admitting_any_cached_catalog() {
        let root =
            std::env::temp_dir().join(format!("sage-discovery-stale-{}", uuid::Uuid::new_v4()));
        let foreign =
            std::env::temp_dir().join(format!("sage-discovery-foreign-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&foreign).unwrap();
        let mut authority = CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                SageProjectId::parse("project-session-stale").unwrap(),
                ProjectGeneration::new(37).unwrap(),
                SageRootId::parse("root-session-stale").unwrap(),
                &root,
            )
            .unwrap(),
        );
        assert!(authority.revalidate().is_ok());
        authority
            .adopt_foreign_root_identity_for_test(&foreign)
            .unwrap();
        assert!(authority.revalidate().is_err());

        let binding_id = BindingId::new("bindingId", "codex-app-server").unwrap();
        let binding_digest = [7; 32];

        let empty = CodexDiscoveryManager::new(startup_codex_binding_registration());
        assert_eq!(
            empty
                .validate_session_model(&authority, &binding_id, binding_digest, None)
                .unwrap_err(),
            CodexDiscoveryRunError::Drift
        );

        let mismatched = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let unrelated_scope = scope();
        let unrelated_epoch = RuntimeEpoch::new(41).unwrap();
        mismatched
            .publish(
                mismatched.gate.current_epoch(),
                CacheKey {
                    scope: unrelated_scope.clone(),
                    runtime_epoch: unrelated_epoch,
                },
                response(&unrelated_scope, unrelated_epoch),
            )
            .unwrap();
        assert_eq!(
            mismatched
                .validate_session_model(&authority, &binding_id, binding_digest, None)
                .unwrap_err(),
            CodexDiscoveryRunError::Drift
        );

        let matching = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let stale_scope = CacheScope {
            project_id: authority.project_id().clone(),
            project_generation: authority.project_generation(),
            root_id: authority.root_id().clone(),
            root_identity: authority.identity_bytes(),
            binding_digest,
        };
        let stale_epoch = RuntimeEpoch::new(43).unwrap();
        matching
            .publish(
                matching.gate.current_epoch(),
                CacheKey {
                    scope: stale_scope.clone(),
                    runtime_epoch: stale_epoch,
                },
                response(&stale_scope, stale_epoch),
            )
            .unwrap();
        assert_eq!(
            matching
                .validate_session_model(&authority, &binding_id, binding_digest, None)
                .unwrap_err(),
            CodexDiscoveryRunError::Drift
        );

        drop(authority);
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(foreign).unwrap();
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
    fn concurrent_matching_discovery_requests_join_and_release_the_operation_gate() {
        let manager = CodexDiscoveryManager::new(startup_codex_binding_registration());
        let request = GetCodexDiscoveryRequest {
            project_id: SageProjectId::parse("project-joined").unwrap(),
            project_generation: ProjectGeneration::new(23).unwrap(),
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
        };
        let root = CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                request.project_id.clone(),
                request.project_generation,
                SageRootId::parse("root-joined").unwrap(),
                &std::env::current_dir().unwrap(),
            )
            .unwrap(),
        );
        let first = manager
            .prepare(
                request.clone(),
                root.clone_for_test(),
                CodexDiscoveryDeadline::new().unwrap(),
            )
            .unwrap();
        let second = manager
            .prepare(request, root, CodexDiscoveryDeadline::new().unwrap())
            .unwrap();
        assert!(first.owns_flight);
        assert!(!second.owns_flight);

        let owner = Arc::clone(&manager);
        let worker = thread::spawn(move || owner.discover(first));
        assert_eq!(
            manager.discover(second),
            Err(CodexDiscoveryRunError::BindingUnavailable)
        );
        assert_eq!(
            worker.join().unwrap(),
            Err(CodexDiscoveryRunError::BindingUnavailable)
        );
        assert!(manager.gate.prepare().is_ok());
    }

    #[cfg(windows)]
    fn admitted_joined_flight(
        label: &str,
        deadline: CodexDiscoveryDeadline,
    ) -> (
        Arc<CodexDiscoveryManager>,
        PreparedCodexDiscovery,
        PreparedCodexDiscovery,
    ) {
        let registration =
            crate::acp::bindings::codex::CodexBindingRegistration::admitted_for_test();
        let manager = CodexDiscoveryManager::new(registration.clone());
        let request = GetCodexDiscoveryRequest {
            project_id: SageProjectId::parse(format!("project-{label}")).unwrap(),
            project_generation: ProjectGeneration::new(29).unwrap(),
            binding_id: BindingId::new("bindingId", crate::acp::bindings::codex::CODEX_BINDING_ID)
                .unwrap(),
        };
        let root = CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                request.project_id.clone(),
                request.project_generation,
                SageRootId::parse(format!("root-{label}")).unwrap(),
                &std::env::current_dir().unwrap(),
            )
            .unwrap(),
        );
        let binding = registration.refresh().unwrap();
        manager.seed_session_validation_for_test(
            &root,
            binding.binding_digest(),
            Some(ModelId::new("modelId", "gpt-joined").unwrap()),
        );
        let owner = manager
            .prepare(request.clone(), root.clone_for_test(), deadline)
            .unwrap();
        let joined = manager.prepare(request, root, deadline).unwrap();
        assert!(owner.owns_flight);
        assert!(!joined.owns_flight);
        (manager, owner, joined)
    }

    #[cfg(windows)]
    #[test]
    fn a_joined_discovery_flight_refuses_a_completed_catalog_after_project_invalidation() {
        let (manager, owner, joined) =
            admitted_joined_flight("joined-invalidated", CodexDiscoveryDeadline::new().unwrap());
        assert!(manager.discover(owner).is_ok());

        manager.invalidate();

        assert_eq!(
            manager.discover(joined).unwrap_err(),
            CodexDiscoveryRunError::Drift
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_joined_discovery_flight_refuses_a_completed_catalog_after_its_own_deadline_expires() {
        let (manager, owner, mut joined) =
            admitted_joined_flight("joined-expired", CodexDiscoveryDeadline::new().unwrap());
        assert!(manager.discover(owner).is_ok());
        // The owner has completed; only the joined caller's own deadline is expired.
        joined.deadline = CodexDiscoveryDeadline::for_test(Duration::ZERO, Duration::ZERO);
        assert_eq!(
            joined.deadline.ensure_total(),
            Err(CodexDiscoveryRunError::Timeout)
        );

        assert_eq!(
            manager.discover(joined).unwrap_err(),
            CodexDiscoveryRunError::Timeout
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

        assert!(manager.cache.response_for(&scope).unwrap().is_none());
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
