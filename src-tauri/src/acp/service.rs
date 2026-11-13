pub(crate) mod prompt_files;
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
            SessionCoordinator, ThreadWaitError,
        },
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
mod browser_attachment;
#[cfg(windows)]
mod browser_mcp;
mod browser_operation;
pub(crate) mod browser_permission;
mod browser_shutdown;
#[cfg(all(windows, feature = "acp-fixture"))]
mod characterization;
mod codex_auth;
mod codex_discovery;
mod codex_launch_failure;
mod codex_live;
mod codex_operation;
mod configuration;
mod debug;
mod intent_snapshot;
mod live_lanes;
mod live_routes;
mod page_bytes;
pub(crate) use live_lanes::{ChatRuntimeRelease, LIVE_RUNTIME_LANES, explicit_close_deadline};
mod permission;
// Only the Pi bridge fixture replay drives decisions through the service from outside it.
#[cfg(all(test, feature = "acp-fixture"))]
pub(crate) use permission::tests::support::{
    admit_permission_request, service_with_pending_requests,
};
mod session;
mod session_close;
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
mod chat_fork;
mod child_transcript;
mod goal;
pub(crate) use child_transcript::{ChildTranscriptRead, ChildTranscriptReadError};
mod renderer_recreation;
mod renderer_recreation_orchestrator;
mod renderer_recreation_packet;
#[cfg(any(debug_assertions, feature = "diagnostics"))]
pub(crate) mod runtime_identity;
mod runtime_slot;
mod session_config;
pub(crate) use session_config::QueueError;
mod mcp;
mod session_control;
mod session_surface;
mod turn_undo;
pub(crate) use session_control::SessionControlError;
pub(crate) use turn_undo::UndoneRelaunchError;
#[cfg(test)]
pub(crate) use turn_undo::stage_turn_undo;
mod session_error;
mod slash_commands;
mod usage;
pub(crate) use session_error::{CodexSessionError, SessionDispatchError};
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
pub(crate) use renderer_recreation::RendererRecreationUsage;
pub(crate) use renderer_recreation_orchestrator::RendererRecreationReloadLifecycle;

use page_bytes::PageBytes;
#[cfg(any(debug_assertions, feature = "diagnostics"))]
use runtime_identity::CodexRuntimeIdentityLedger;
use runtime_slot::RuntimeSlot;

pub(crate) use crate::acp::intent::PromptSendError;

pub(crate) struct AcpService {
    pub(crate) file_stages: Arc<Mutex<crate::acp::attached_context::files::FileStages>>,
    pub(crate) file_stage_reader: Arc<tokio::sync::Mutex<()>>,
    database: Database,
    journal: JournalHandle,
    journal_thread: Mutex<Option<BoundedThread>>,
    /// Starts durable sessions; it never reserves a runtime tree.
    session_starts: SessionCoordinator,
    lanes: Box<[live_lanes::LiveLane]>,
    idle_release: live_lanes::IdleRelease,
    publisher: Arc<dyn ProjectionPublisher>,
    notifier: Option<Arc<ProjectionNotifier>>,
    page_reads: PageReadAdmission,
    read_text_files: ReadTextFileService,
    browser_permissions: Mutex<
        std::collections::HashMap<
            crate::acp::contract::SageServerRequestId,
            browser_permission::PendingBrowserPermission,
        >,
    >,
    browser_attachments: crate::acp::browser::attachment::BrowserAttachments,
    #[cfg(windows)]
    browser_app: Mutex<Option<tauri::AppHandle>>,
    registry: Arc<BindingRegistry>,
    codex_gate: Arc<codex_operation::CodexOperationGate>,
    codex_discovery: Arc<CodexDiscoveryManager>,
    codex_auth: Arc<CodexAuthManager>,
    #[cfg(any(debug_assertions, feature = "diagnostics"))]
    codex_runtime_identity: Arc<CodexRuntimeIdentityLedger>,
    #[cfg(test)]
    codex_runtime_release_fail_once: AtomicBool,
    /// Chat releases refused because another holder had the lane.
    #[cfg(test)]
    lane_busy_refusals: std::sync::atomic::AtomicUsize,
    /// Chat releases refused because the harness was answering a control.
    #[cfg(test)]
    control_running_refusals: std::sync::atomic::AtomicUsize,
    /// When set, the next lane-busy refusal lets go of the lane's claim, as
    /// the holder finishing would.
    #[cfg(test)]
    yield_lane_on_busy: AtomicBool,
    #[cfg(test)]
    codex_runtime_release_fail_always: AtomicBool,
    #[cfg(all(windows, feature = "acp-fixture"))]
    characterization: Option<CharacterizationBinding>,
    #[cfg(all(test, windows))]
    last_shutdown_stage: Mutex<&'static str>,
    #[cfg(all(test, windows))]
    last_cancel_stage: Mutex<&'static str>,
    #[cfg(test)]
    runtime_port_finish_park_failure: Mutex<bool>,
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
        self.registry.codex_admission().clone()
    }

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
            Arc::clone(
                binding
                    .codex_binding()
                    .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?,
            ),
            project_root,
            runtime_epoch,
            session_id,
        )
        .inspect_err(|_| {
            self.registry.codex_admission().invalidate_drift();
        })
    }

    pub(crate) fn codex_discovery_manager(&self) -> Arc<CodexDiscoveryManager> {
        Arc::clone(&self.codex_discovery)
    }

    pub(crate) fn codex_auth_manager(&self) -> Arc<CodexAuthManager> {
        Arc::clone(&self.codex_auth)
    }

    #[cfg(test)]
    pub(crate) fn hold_live_operation_for_test(&self) -> impl Sized {
        let prepared = self
            .codex_gate
            .prepare_live()
            .expect("a fresh gate admits one live operation");
        self.codex_gate
            .begin(&prepared)
            .expect("a prepared live operation begins")
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
        self.session_operation_binding(requested)?;
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

    fn session_operation_binding(
        &self,
        requested: &BindingId,
    ) -> Result<&crate::acp::bindings::registry::BindingRecord, RegistryUnavailable> {
        let binding = self.registry.route(requested.as_str())?;
        binding
            .session_operations_enabled()
            .then_some(binding)
            .ok_or(RegistryUnavailable::UnknownBinding)
    }

    pub(crate) fn dispatch_session_turn(
        &self,
        binding_id: &BindingId,
        intent: crate::acp::runtime::HostPromptIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, PromptSendError<SessionDispatchError>> {
        self.resolve_session_binding(binding_id)
            .map_err(|_| PromptSendError::not_started(SessionDispatchError::Binding))?
            .dispatch_turn(self, intent)
            .map_err(|failure| failure.map(SessionDispatchError::Operation))
    }

    pub(crate) fn dispatch_session_cancel(
        &self,
        binding_id: &BindingId,
        intent: crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, SessionDispatchError> {
        self.session_operation_binding(binding_id)
            .map_err(|_| SessionDispatchError::Binding)?
            .dispatch_cancel(self, intent)
            .map_err(SessionDispatchError::Operation)
    }

    pub(crate) fn reserve_page_read(
        &self,
        project_id: &SageProjectId,
        chat_id: &SageChatId,
    ) -> Result<PageReadLease, PageAdmissionError> {
        self.page_reads.try_acquire(project_id, chat_id)
    }

    /// Reads a chat page from SQLite. When called from async code on a
    /// multi-thread runtime, the worker's other tasks move to another thread
    /// while it runs.
    pub(crate) fn load_chat_page(
        &self,
        project_id: SageProjectId,
        chat_id: SageChatId,
        cursor: Option<PageCursor>,
    ) -> Result<AcpChatPage, StoreError> {
        crate::blocking::in_place(|| {
            let mut page = load_chat_page(
                &self.database.connection(),
                project_id.clone(),
                chat_id.clone(),
                cursor.clone(),
            )?;
            self.compose_session_config(&mut page)?;
            let mut size = PageBytes::measure(&page)?;
            // History yields room for the session config that does not fit.
            if let Some(reserve) = session_config::config_reserve(&mut page, &size)? {
                page = crate::acp::store::load_chat_page_reserving(
                    &self.database.connection(),
                    project_id,
                    chat_id,
                    cursor,
                    reserve,
                )?;
                self.compose_session_config(&mut page)?;
                size = PageBytes::measure(&page)?;
            }
            self.fit_session_config_to_page_budget(&mut page, &size)?;
            self.compose_goal(&mut page);
            self.compose_usage(&mut page);
            self.fit_goal_to_page_budget(&mut page, &size)?;
            self.compose_session_surface(&mut page)?;
            Ok(page)
        })
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

    #[cfg(test)]
    pub(crate) fn install_runtime_for_test(
        &self,
        owner: crate::acp::scheduler::RuntimeOwner,
        runtime: Arc<crate::acp::runtime::RuntimeHost>,
    ) {
        self.lane_at(0)
            .runtime
            .claim(owner, runtime)
            .expect("empty test slot");
    }
}

#[cfg(test)]
mod tests;
