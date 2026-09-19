use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    acp::{
        bindings::registry::{AdmittedBindingRecord, BindingRegistry, RegistryUnavailable},
        contract::{ProjectGeneration, SageChatId, SageProjectId},
        intent::{AcpChatPage, BindingId, IntentSnapshot, PageCursor},
        page_admission::{PageAdmissionError, PageReadAdmission, PageReadLease},
        runtime::{
            BoundedThread, JournalHandle, ProjectionNotifier, ProjectionPublisher,
            SessionCoordinator, SessionCoordinatorError, ThreadWaitError,
        },
        scheduler::Scheduler,
        store::{StoreError, load_chat_page},
    },
    persistence::Database,
};

#[cfg(test)]
use crate::acp::bindings::codex::CodexBindingRegistration;

#[cfg(all(test, windows))]
use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        codex::{CodexProjectRootAuthority, CodexRuntime},
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
};

mod bootstrap;
#[cfg(all(windows, feature = "acp-fixture"))]
mod characterization;
mod codex_auth;
mod codex_discovery;
mod codex_launch_failure;
mod codex_live;
mod codex_operation;
mod codex_session;
mod configuration;
mod intent_snapshot;
mod permission;
pub(crate) use permission::PermissionDecisionIntent;
mod file_snapshot;
pub(crate) use file_snapshot::verify_current_owner;
mod project_switch;
mod read_text_file;
#[cfg(test)]
pub(crate) use read_text_file::{DiskAllowedState, ReadTextClassification, ReadTextFlushState};
pub(crate) use read_text_file::{
    ReadTextBegin, ReadTextFileService, ReadTextFlushAck, ReadTextNextAction, ReadTextOwner,
};
#[allow(dead_code)] // Fixed production observation boundary; lifecycle wiring is separate.
mod renderer_recreation;
mod renderer_recreation_orchestrator;
#[allow(dead_code)] // Fixed packet schema; no generic observer is exposed.
mod renderer_recreation_packet;
#[cfg(any(debug_assertions, feature = "diagnostics"))]
pub(crate) mod runtime_identity;
mod runtime_slot;
mod session_config;
mod shutdown;

#[cfg(all(windows, feature = "acp-fixture"))]
pub(crate) use characterization::{
    BINDING_DIGEST_HEX as CHARACTERIZATION_BINDING_DIGEST,
    BINDING_ID as CHARACTERIZATION_BINDING_ID, CharacterizationArtifact, CharacterizationBinding,
    CharacterizationCase, CharacterizationError,
};
pub(crate) use codex_auth::{CodexAuthAction, CodexAuthManager};
use codex_discovery::CodexDiscoveryManager;
pub(crate) use codex_launch_failure::CodexLaunchSetupError;
use configuration::ServiceConfiguration;
use intent_snapshot::load_intent_snapshot;
use renderer_recreation::RendererRecreationNativeSnapshot;
pub(crate) use renderer_recreation_orchestrator::RendererRecreationReloadLifecycle;
#[cfg(any(debug_assertions, feature = "diagnostics"))]
use runtime_identity::CodexRuntimeIdentityLedger;
use runtime_slot::RuntimeSlot;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodexSessionError {
    #[error("the exact Codex binding is unavailable")]
    Binding(crate::acp::bindings::codex::CodexAdmissionUnavailable),
    #[error("the Codex session intent is stale or foreign")]
    Intent,
    #[error(transparent)]
    Session(#[from] SessionCoordinatorError),
    #[error("Codex runtime capacity is occupied")]
    Capacity,
    #[error("Codex live runtime requires recovery")]
    Recovery,
    #[error("Codex process launch failed: {0:?}")]
    LaunchFailure(crate::acp::process::LaunchError),
    #[error("Codex process launch setup failed: {0:?}")]
    LaunchSetup(CodexLaunchSetupError),
}

pub(crate) enum SessionDispatchError {
    Binding,
    Operation(CodexSessionError),
}

/// Carries positive pre-invocation proof without inferring it from an error kind.
#[derive(Debug)]
pub(crate) struct PromptSendError<E> {
    pub error: E,
    pub prompt_dispatch: crate::acp::intent::PromptDispatch,
}

impl<E> PromptSendError<E> {
    pub(crate) fn not_started(error: E) -> Self {
        Self {
            error,
            prompt_dispatch: crate::acp::intent::PromptDispatch::NotStarted,
        }
    }

    pub(crate) fn unknown(error: E) -> Self {
        Self {
            error,
            prompt_dispatch: crate::acp::intent::PromptDispatch::Unknown,
        }
    }

    pub(crate) fn map<F>(self, map: impl FnOnce(E) -> F) -> PromptSendError<F> {
        PromptSendError {
            error: map(self.error),
            prompt_dispatch: self.prompt_dispatch,
        }
    }
}

pub(crate) struct AcpService {
    database: Database,
    scheduler: Scheduler,
    journal: JournalHandle,
    journal_thread: Mutex<Option<BoundedThread>>,
    sessions: SessionCoordinator,
    runtime: Arc<RuntimeSlot>,
    publisher: Arc<dyn ProjectionPublisher>,
    notifier: Option<Arc<ProjectionNotifier>>,
    page_reads: PageReadAdmission,
    read_text_files: ReadTextFileService,
    registry: Arc<BindingRegistry>,
    codex_gate: Arc<codex_operation::CodexOperationGate>,
    codex_discovery: Arc<CodexDiscoveryManager>,
    codex_auth: Arc<CodexAuthManager>,
    codex_recovery: Arc<Mutex<Option<codex_live::PendingCodexRecovery>>>,
    #[cfg(any(debug_assertions, feature = "diagnostics"))]
    codex_runtime_identity: Arc<CodexRuntimeIdentityLedger>,
    #[cfg(test)]
    codex_runtime_release_fail_once: AtomicBool,
    #[cfg(test)]
    codex_runtime_release_fail_always: AtomicBool,
    #[cfg(all(windows, feature = "acp-fixture"))]
    characterization: Option<CharacterizationBinding>,
    #[cfg(all(test, windows))]
    last_shutdown_stage: Mutex<&'static str>,
    #[cfg(all(test, windows))]
    last_cancel_stage: Mutex<&'static str>,
}

impl AcpService {
    /// Future admitted ACP pumps own path mapping and event delivery; this
    /// narrow service owns only the bounded read/ack linearization state.
    pub(crate) fn read_text_file_service(&self) -> &ReadTextFileService {
        &self.read_text_files
    }

    #[cfg(all(test, windows))]
    pub(crate) fn last_shutdown_stage(&self) -> &'static str {
        *self
            .last_shutdown_stage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(all(test, windows))]
    pub(crate) fn last_cancel_stage(&self) -> &'static str {
        *self
            .last_cancel_stage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(all(test, windows))]
    fn record_shutdown_stage(&self, stage: &'static str) {
        *self
            .last_shutdown_stage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = stage;
    }

    #[cfg(all(test, windows))]
    pub(super) fn record_cancel_stage(&self, stage: &'static str) {
        *self
            .last_cancel_stage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = stage;
    }

    #[cfg(test)]
    pub(crate) fn codex_registration(&self) -> CodexBindingRegistration {
        self.registry.codex().admission().clone()
    }

    #[cfg(test)]
    pub(crate) fn binding_registry(&self) -> &Arc<BindingRegistry> {
        &self.registry
    }

    #[cfg(all(test, windows))]
    pub(crate) fn codex_runtime(
        &self,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<CodexRuntime, AdapterRuntimeError> {
        let binding = self
            .refresh_codex_binding()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        CodexRuntime::from_admitted_binding(
            Arc::clone(binding.binding()),
            project_root,
            runtime_epoch,
            session_id,
        )
        .inspect_err(|_| {
            self.registry.codex().admission().invalidate_drift();
        })
    }

    pub(crate) fn codex_discovery_manager(&self) -> Arc<CodexDiscoveryManager> {
        Arc::clone(&self.codex_discovery)
    }

    pub(crate) fn codex_auth_manager(&self) -> Arc<CodexAuthManager> {
        Arc::clone(&self.codex_auth)
    }

    #[cfg(any(debug_assertions, feature = "diagnostics"))]
    pub(crate) fn codex_runtime_identity(
        &self,
    ) -> Option<runtime_identity::CodexRuntimeIdentityDiagnostics> {
        self.codex_runtime_identity.observation()
    }

    #[cfg(test)]
    pub(crate) fn accepts_codex_binding(&self, requested: &BindingId) -> bool {
        self.refresh_codex_binding()
            .is_ok_and(|binding| binding.binding_id() == requested.as_str())
    }

    /// Fixed provider lookup only. Requested session binding IDs must resolve
    /// through `resolve_session_binding`, where unknown IDs remain hard failures.
    #[cfg(test)]
    pub(super) fn refresh_codex_binding(
        &self,
    ) -> Result<AdmittedBindingRecord<'_>, crate::acp::bindings::codex::CodexAdmissionUnavailable>
    {
        self.registry
            .resolve(self.registry.codex().binding_id())
            .map_err(|error| match error {
                RegistryUnavailable::Admission(reason) => reason,
                RegistryUnavailable::UnknownBinding => {
                    crate::acp::bindings::codex::CodexAdmissionUnavailable::IndependentAdmission
                }
            })
            .inspect_err(|_| {
                self.codex_discovery.invalidate();
                self.codex_auth.invalidate();
            })
    }

    pub(crate) fn resolve_session_binding(
        &self,
        requested: &BindingId,
    ) -> Result<AdmittedBindingRecord<'_>, RegistryUnavailable> {
        self.registry
            .resolve(requested.as_str())
            .inspect_err(|error| {
                if matches!(
                    error,
                    RegistryUnavailable::Admission(
                        crate::acp::bindings::codex::CodexAdmissionUnavailable::Drift
                    )
                ) {
                    self.codex_discovery.invalidate();
                    self.codex_auth.invalidate();
                }
            })
    }

    pub(crate) fn dispatch_session_turn(
        &self,
        binding_id: &BindingId,
        command: crate::acp::store::PrepareTurnCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, PromptSendError<SessionDispatchError>> {
        self.resolve_session_binding(binding_id)
            .map_err(|_| PromptSendError::not_started(SessionDispatchError::Binding))?
            .dispatch_turn(self, command)
            .map_err(|failure| failure.map(SessionDispatchError::Operation))
    }

    pub(crate) fn dispatch_session_cancel(
        &self,
        binding_id: &BindingId,
        intent: crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, SessionDispatchError> {
        self.registry
            .route(binding_id.as_str())
            .map_err(|_| SessionDispatchError::Binding)?
            .dispatch_cancel(self, intent)
            .map_err(SessionDispatchError::Operation)
    }

    pub(crate) fn dispatch_session_close(
        &self,
        binding_id: &BindingId,
        command: crate::acp::store::CloseSessionCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, SessionDispatchError> {
        self.registry
            .route(binding_id.as_str())
            .map_err(|_| SessionDispatchError::Binding)?
            .dispatch_close(self, command)
            .map_err(SessionDispatchError::Operation)
    }

    pub(crate) fn reserve_page_read(
        &self,
        project_id: &SageProjectId,
        chat_id: &SageChatId,
    ) -> Result<PageReadLease, PageAdmissionError> {
        self.page_reads.try_acquire(project_id, chat_id)
    }

    pub(crate) fn load_chat_page(
        &self,
        project_id: SageProjectId,
        chat_id: SageChatId,
        cursor: Option<PageCursor>,
    ) -> Result<AcpChatPage, StoreError> {
        let mut page = load_chat_page(&self.database.connection(), project_id, chat_id, cursor)?;
        self.compose_session_config(&mut page)?;
        Ok(page)
    }

    pub(crate) fn intent_snapshot(
        &self,
        project_id: SageProjectId,
        project_generation: ProjectGeneration,
        chat_id: SageChatId,
    ) -> Result<IntentSnapshot, StoreError> {
        load_intent_snapshot(&self.database, project_id, project_generation, chat_id)
    }

    pub(crate) fn acknowledge_projection_bootstrap(&self) {
        if let Some(notifier) = &self.notifier {
            let _ = notifier.acknowledge_authoritative_bootstrap();
        }
    }

    #[allow(dead_code)] // Invoked by the fixed, internal renderer-recreation path only.
    pub(crate) fn renderer_recreation_native_snapshot(
        &self,
        project_id: &SageProjectId,
        chat_id: &SageChatId,
    ) -> Result<RendererRecreationNativeSnapshot, StoreError> {
        let (live_trees, live_descendants) =
            crate::acp::process::TreeCapacityPool::global().observation_usage();
        RendererRecreationNativeSnapshot::capture(
            &self.database.connection(),
            project_id,
            chat_id,
            u8::from(self.runtime.is_active()),
            live_trees,
            live_descendants,
        )
    }

    #[cfg(test)]
    pub(crate) fn install_runtime_for_test(
        &self,
        owner: crate::acp::scheduler::RuntimeOwner,
        runtime: Arc<crate::acp::runtime::RuntimeHost>,
    ) {
        self.runtime.claim(owner, runtime).expect("empty test slot");
    }
}

impl std::fmt::Debug for AcpService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AcpService")
            .field("database", &self.database)
            .field("scheduler", &self.scheduler)
            .field("journal", &self.journal)
            .field("sessions", &self.sessions)
            .field("runtime_active", &self.runtime.is_active())
            .field(
                "codex_admitted",
                &self.registry.codex().admission().is_available(),
            )
            .field("notification_active", &self.notifier.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
