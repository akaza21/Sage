use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use crate::acp::{
    bindings::codex::{
        CodexAuthDeadline, CodexAuthOutcome, CodexAuthProtocol, CodexBindingRegistration,
        CodexDiscoveryRunError, CodexProjectRootAuthority, NativeApiKey, NativeBrowser,
        ProductionAuthTransport, run_account_protocol, run_api_key_protocol, run_login_protocol,
        run_logout_protocol,
    },
    ipc::{CodexAuthPhase, CodexAuthResponse, CodexAuthScope},
};

use super::codex_operation::{CodexOperationGate, PreparedCodexOperation};

#[derive(Debug)]
pub(crate) enum CodexAuthAction {
    Observe,
    Login,
    ApiKey(NativeApiKey),
    Logout,
}

impl CodexAuthAction {
    pub(crate) fn changes_account(&self) -> bool {
        !matches!(self, Self::Observe)
    }
}

pub(crate) struct PreparedCodexAuth {
    scope: CodexAuthScope,
    root: CodexProjectRootAuthority,
    action: CodexAuthAction,
    deadline: CodexAuthDeadline,
    operation: PreparedCodexOperation,
}

#[derive(Debug, Clone, Copy)]
struct AuthState {
    phase: CodexAuthPhase,
    revision: u64,
}

struct ActiveCancel {
    scope: CodexAuthScope,
    flag: Arc<AtomicBool>,
}

pub(crate) struct CodexAuthManager {
    registration: CodexBindingRegistration,
    gate: Arc<CodexOperationGate>,
    state: Mutex<AuthState>,
    cancel: Mutex<Option<ActiveCancel>>,
    next_revision: AtomicU64,
}

impl CodexAuthManager {
    pub(super) fn new(
        registration: CodexBindingRegistration,
        gate: Arc<CodexOperationGate>,
    ) -> Arc<Self> {
        Arc::new(Self {
            registration,
            gate,
            state: Mutex::new(AuthState {
                phase: CodexAuthPhase::SignedOut,
                revision: 1,
            }),
            cancel: Mutex::new(None),
            next_revision: AtomicU64::new(2),
        })
    }

    pub(crate) fn snapshot(&self, scope: CodexAuthScope) -> CodexAuthResponse {
        let state = *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        response(scope, state)
    }

    pub(crate) fn prepare(
        &self,
        scope: CodexAuthScope,
        root: CodexProjectRootAuthority,
        action: CodexAuthAction,
        consent: bool,
        deadline: CodexAuthDeadline,
    ) -> Result<PreparedCodexAuth, CodexDiscoveryRunError> {
        if root.project_id() != &scope.project_id
            || root.project_generation() != scope.project_generation
        {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        if matches!(action, CodexAuthAction::Login) && !consent {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        let operation = self.gate.prepare()?;
        Ok(PreparedCodexAuth {
            scope,
            root,
            action,
            deadline,
            operation,
        })
    }

    pub(crate) fn run(
        self: &Arc<Self>,
        prepared: PreparedCodexAuth,
    ) -> Result<CodexAuthResponse, CodexDiscoveryRunError> {
        let changes_account = prepared.action.changes_account();
        let operation_epoch = CodexOperationGate::prepared_epoch(&prepared.operation);
        let active = self.gate.begin(&prepared.operation)?;
        let result = self.run_active(prepared, &active);
        let settlement = if result == Err(CodexDiscoveryRunError::Cleanup) {
            Err(CodexDiscoveryRunError::Cleanup)
        } else {
            Ok(())
        };
        active.settle(settlement);
        self.clear_cancel();
        if result.is_err() && changes_account && self.gate.is_current(operation_epoch) {
            self.publish(CodexAuthPhase::Recovery);
        }
        result
    }

    fn run_active(
        &self,
        prepared: PreparedCodexAuth,
        active: &super::codex_operation::ActiveCodexOperation,
    ) -> Result<CodexAuthResponse, CodexDiscoveryRunError> {
        let binding = self.registration.refresh().map_err(|error| match error {
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
        })?;
        if prepared.scope.binding_id.as_str() != binding.binding_id() {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        prepared
            .root
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        let runtime_epoch = self.gate.allocate_runtime_epoch()?;
        let cancel = Arc::new(AtomicBool::new(false));
        if matches!(prepared.action, CodexAuthAction::Login) {
            self.install_cancel(prepared.scope.clone(), Arc::clone(&cancel))?;
            self.publish(CodexAuthPhase::Waiting);
        }
        let mut protocol = CodexAuthProtocol::from_admitted_binding(&binding, runtime_epoch);
        let mut transport = ProductionAuthTransport::launch(
            binding,
            prepared.root,
            runtime_epoch,
            prepared.deadline,
            cancel,
            |control| active.register_control(control),
        )?;
        let outcome = match prepared.action {
            CodexAuthAction::Observe => run_account_protocol(
                &mut protocol,
                &mut transport,
                runtime_epoch.get(),
                prepared.deadline,
            ),
            CodexAuthAction::Login => run_login_protocol(
                &mut protocol,
                &mut transport,
                &NativeBrowser,
                runtime_epoch.get(),
                prepared.deadline,
            ),
            CodexAuthAction::ApiKey(key) => run_api_key_protocol(
                &mut protocol,
                &mut transport,
                &key,
                runtime_epoch.get(),
                prepared.deadline,
            ),
            CodexAuthAction::Logout => run_logout_protocol(
                &mut protocol,
                &mut transport,
                runtime_epoch.get(),
                prepared.deadline,
            ),
        }?;
        if !self
            .gate
            .is_current(CodexOperationGate::prepared_epoch(&prepared.operation))
        {
            return Err(CodexDiscoveryRunError::Drift);
        }
        transport.revalidate()?;
        let phase = match outcome {
            CodexAuthOutcome::SignedIn => CodexAuthPhase::SignedIn,
            CodexAuthOutcome::SignedOut | CodexAuthOutcome::Cancelled => CodexAuthPhase::SignedOut,
        };
        let state = self.publish(phase);
        Ok(response(prepared.scope, state))
    }

    pub(crate) fn cancel(
        &self,
        scope: CodexAuthScope,
    ) -> Result<CodexAuthResponse, CodexDiscoveryRunError> {
        let slot = self
            .cancel
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        let active = slot.as_ref().ok_or(CodexDiscoveryRunError::InvalidScope)?;
        if active.scope != scope {
            return Err(CodexDiscoveryRunError::InvalidScope);
        }
        active.flag.store(true, Ordering::Release);
        drop(slot);
        Ok(self.snapshot(scope))
    }

    fn install_cancel(
        &self,
        scope: CodexAuthScope,
        cancel: Arc<AtomicBool>,
    ) -> Result<(), CodexDiscoveryRunError> {
        let mut slot = self
            .cancel
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        if slot.is_some() {
            return Err(CodexDiscoveryRunError::Busy);
        }
        *slot = Some(ActiveCancel {
            scope,
            flag: cancel,
        });
        Ok(())
    }

    fn clear_cancel(&self) {
        *self
            .cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    fn publish(&self, phase: CodexAuthPhase) -> AuthState {
        let state = AuthState {
            phase,
            revision: self.next_revision.fetch_add(1, Ordering::AcqRel),
        };
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
        state
    }

    pub(crate) fn invalidate(&self) {
        if let Some(active) = self
            .cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            active.flag.store(true, Ordering::Release);
        }
        self.clear_cancel();
        self.publish(CodexAuthPhase::SignedOut);
    }
}

fn response(scope: CodexAuthScope, state: AuthState) -> CodexAuthResponse {
    CodexAuthResponse {
        project_id: scope.project_id,
        project_generation: scope.project_generation,
        binding_id: scope.binding_id,
        phase: state.phase,
        revision: state.revision.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        acp::{
            bindings::codex::{CodexDiscoveryDeadline, startup_codex_binding_registration},
            contract::{ProjectGeneration, SageProjectId, SageRootId},
            intent::BindingId,
            ipc::GetCodexDiscoveryRequest,
            service::codex_discovery::CodexDiscoveryManager,
        },
        project_registry::ProjectRootAuthority,
    };
    use std::path::Path;

    #[test]
    fn state_projection_is_sanitized_monotonic_and_memory_only() {
        let gate = CodexOperationGate::new();
        let manager = CodexAuthManager::new(startup_codex_binding_registration(), gate);
        let scope = scope();
        let signed_out = manager.snapshot(scope.clone());
        assert_eq!(signed_out.phase, CodexAuthPhase::SignedOut);
        assert_eq!(signed_out.revision, "1");

        let waiting = response(scope.clone(), manager.publish(CodexAuthPhase::Waiting));
        let signed_in = response(scope, manager.publish(CodexAuthPhase::SignedIn));
        assert_eq!(waiting.revision, "2");
        assert_eq!(signed_in.revision, "3");
        let encoded = serde_json::to_string(&signed_in).unwrap();
        for forbidden in [
            "authUrl",
            "loginId",
            "email",
            "planType",
            "commandLine",
            "path",
        ] {
            assert!(!encoded.contains(forbidden));
        }
    }

    #[test]
    fn login_requires_explicit_consent_and_exact_project_scope() {
        let gate = CodexOperationGate::new();
        let manager = CodexAuthManager::new(startup_codex_binding_registration(), gate);
        let scope = scope();
        let authority = root(&scope);
        assert!(matches!(
            manager.prepare(
                scope.clone(),
                authority,
                CodexAuthAction::Login,
                false,
                CodexAuthDeadline::new().unwrap()
            ),
            Err(CodexDiscoveryRunError::InvalidScope)
        ));

        let mut foreign = scope.clone();
        foreign.project_id = SageProjectId::parse("project-foreign").unwrap();
        assert!(matches!(
            manager.prepare(
                foreign,
                root(&scope),
                CodexAuthAction::Observe,
                false,
                CodexAuthDeadline::new().unwrap()
            ),
            Err(CodexDiscoveryRunError::InvalidScope)
        ));
    }

    #[test]
    fn auth_and_discovery_share_one_non_waiting_admission() {
        let gate = CodexOperationGate::new();
        let registration = startup_codex_binding_registration();
        let auth = CodexAuthManager::new(registration.clone(), Arc::clone(&gate));
        let discovery = CodexDiscoveryManager::with_gate(registration, gate);
        let scope = scope();
        let prepared = discovery
            .prepare(
                GetCodexDiscoveryRequest {
                    project_id: scope.project_id.clone(),
                    project_generation: scope.project_generation,
                    binding_id: scope.binding_id.clone(),
                },
                root(&scope),
                CodexDiscoveryDeadline::new().unwrap(),
            )
            .unwrap();
        assert!(matches!(
            auth.prepare(
                scope.clone(),
                root(&scope),
                CodexAuthAction::Observe,
                false,
                CodexAuthDeadline::new().unwrap()
            ),
            Err(CodexDiscoveryRunError::Busy)
        ));
        drop(prepared);
        assert!(
            auth.prepare(
                scope.clone(),
                root(&scope),
                CodexAuthAction::Observe,
                false,
                CodexAuthDeadline::new().unwrap()
            )
            .is_ok()
        );
    }

    #[test]
    fn explicit_cancel_sets_only_the_native_flag_and_invalidation_forgets_waiting() {
        let gate = CodexOperationGate::new();
        let manager = CodexAuthManager::new(startup_codex_binding_registration(), gate);
        let flag = Arc::new(AtomicBool::new(false));
        manager.install_cancel(scope(), Arc::clone(&flag)).unwrap();
        manager.publish(CodexAuthPhase::Waiting);

        let waiting = manager.cancel(scope()).unwrap();
        assert_eq!(waiting.phase, CodexAuthPhase::Waiting);
        assert!(flag.load(Ordering::Acquire));

        let mut foreign = scope();
        foreign.project_generation = ProjectGeneration::new(10).unwrap();
        assert_eq!(
            manager.cancel(foreign),
            Err(CodexDiscoveryRunError::InvalidScope)
        );

        manager.invalidate();
        assert_eq!(manager.snapshot(scope()).phase, CodexAuthPhase::SignedOut);
        assert!(manager.cancel.lock().unwrap().is_none());
    }

    #[test]
    fn effect_unknown_phase_is_memory_only_and_later_observation_can_replace_it() {
        let gate = CodexOperationGate::new();
        let manager = CodexAuthManager::new(startup_codex_binding_registration(), gate);
        let recovery = manager.publish(CodexAuthPhase::Recovery);
        assert_eq!(manager.snapshot(scope()).phase, CodexAuthPhase::Recovery);

        let observed = manager.publish(CodexAuthPhase::SignedIn);
        assert!(observed.revision > recovery.revision);
        assert_eq!(manager.snapshot(scope()).phase, CodexAuthPhase::SignedIn);
    }

    fn scope() -> CodexAuthScope {
        CodexAuthScope {
            project_id: SageProjectId::parse("project-auth").unwrap(),
            project_generation: ProjectGeneration::new(9).unwrap(),
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
        }
    }

    fn root(scope: &CodexAuthScope) -> CodexProjectRootAuthority {
        CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                scope.project_id.clone(),
                scope.project_generation,
                SageRootId::parse("root-auth").unwrap(),
                Path::new(env!("CARGO_MANIFEST_DIR")),
            )
            .unwrap(),
        )
    }
}
