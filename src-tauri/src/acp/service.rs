use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    acp::{
        bindings::codex::CodexBindingRegistration,
        contract::{ProjectGeneration, SageChatId, SageProjectId},
        intent::{AcpChatPage, BindingId, IntentSnapshot, PageCursor},
        page_admission::{PageAdmissionError, PageReadAdmission, PageReadLease},
        runtime::{
            BoundedThread, JournalError, JournalHandle, JournalLane, ProjectionNotifier,
            ProjectionPublisher, SessionCoordinator, SessionCoordinatorError, ThreadWaitError,
        },
        scheduler::Scheduler,
        store::{DatabaseAcpStore, StoreError, StoreMutation, StoreMutationResult, load_chat_page},
    },
    persistence::Database,
};

#[cfg(test)]
use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        codex::{CodexProjectRootAuthority, CodexRuntime},
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
};

#[cfg(all(windows, feature = "acp-fixture"))]
mod characterization;
mod codex_auth;
mod codex_discovery;
mod codex_live;
mod codex_operation;
mod codex_session;
mod configuration;
mod intent_snapshot;
#[cfg(any(test, feature = "acp-fixture"))]
#[allow(dead_code)] // The packaged reload orchestrator consumes this next.
mod renderer_recreation;
mod runtime_slot;
mod shutdown;

#[cfg(all(windows, feature = "acp-fixture"))]
pub(crate) use characterization::{
    BINDING_DIGEST_HEX as CHARACTERIZATION_BINDING_DIGEST,
    BINDING_ID as CHARACTERIZATION_BINDING_ID, CharacterizationArtifact, CharacterizationBinding,
    CharacterizationCase, CharacterizationError,
};
pub(crate) use codex_auth::{CodexAuthAction, CodexAuthManager};
use codex_discovery::CodexDiscoveryManager;
use configuration::ServiceConfiguration;
use intent_snapshot::load_intent_snapshot;
#[cfg(any(test, feature = "acp-fixture"))]
use renderer_recreation::RendererRecreationNativeSnapshot;
use runtime_slot::RuntimeSlot;
use shutdown::map_discovery_shutdown;

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
    #[error("Codex process launch failed")]
    Launch,
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
    codex: CodexBindingRegistration,
    codex_gate: Arc<codex_operation::CodexOperationGate>,
    codex_discovery: Arc<CodexDiscoveryManager>,
    codex_auth: Arc<CodexAuthManager>,
    codex_recovery: Arc<Mutex<Option<codex_live::PendingCodexRecovery>>>,
    #[cfg(test)]
    codex_runtime_release_fail_once: AtomicBool,
    #[cfg(test)]
    codex_runtime_release_fail_always: AtomicBool,
    #[cfg(all(windows, feature = "acp-fixture"))]
    characterization: Option<CharacterizationBinding>,
}

impl AcpService {
    fn build(
        database: Database,
        publisher: Arc<dyn ProjectionPublisher>,
        notifier: Option<Arc<ProjectionNotifier>>,
        configuration: ServiceConfiguration,
    ) -> Result<Self, StoreError> {
        let store = DatabaseAcpStore::new(database.clone());
        let (journal, runner) = JournalHandle::new(store);
        let mut journal_thread =
            BoundedThread::spawn("sage-acp-journal", move || runner.run_blocking())
                .map_err(|_| StoreError::Corrupt("journal thread start"))?;
        let recovery = journal
            .enqueue_one(
                JournalLane::Control,
                StoreMutation::RecoverAfterRestart {
                    now_millis: crate::persistence::now_millis(),
                },
                u64::try_from(crate::persistence::now_millis())
                    .unwrap_or_default()
                    .saturating_add(30_000),
            )
            .and_then(|ticket| ticket.blocking_committed())
            .map_err(journal_store_error);
        if !matches!(
            recovery.as_ref().map(|commit| commit.results.as_slice()),
            Ok([StoreMutationResult::Recovery(_)])
        ) {
            journal.shutdown();
            let _ = journal_thread.wait(Duration::from_millis(
                crate::acp::limits::LIMITS.deadlines_ms.explicit_close,
            ));
            return Err(recovery
                .err()
                .unwrap_or(StoreError::Corrupt("recovery journal result")));
        }
        let scheduler = Scheduler::new();
        let sessions =
            SessionCoordinator::new(journal.clone(), scheduler.clone(), Arc::clone(&publisher));
        if let Some(notifier) = &notifier
            && let Ok(Some(project_id)) = database.active_project_id()
            && let Ok(project_id) = SageProjectId::parse(project_id)
        {
            notifier.set_active_project(project_id);
        }
        let codex_gate = codex_operation::CodexOperationGate::new();
        let codex_discovery =
            CodexDiscoveryManager::with_gate(configuration.codex.clone(), Arc::clone(&codex_gate));
        let codex_auth =
            CodexAuthManager::new(configuration.codex.clone(), Arc::clone(&codex_gate));
        Ok(Self {
            database,
            scheduler,
            journal,
            journal_thread: Mutex::new(Some(journal_thread)),
            sessions,
            runtime: Arc::new(RuntimeSlot::new()),
            publisher,
            notifier,
            page_reads: PageReadAdmission::default(),
            codex: configuration.codex,
            codex_gate,
            codex_discovery,
            codex_auth,
            codex_recovery: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            codex_runtime_release_fail_once: AtomicBool::new(false),
            #[cfg(test)]
            codex_runtime_release_fail_always: AtomicBool::new(false),
            #[cfg(all(windows, feature = "acp-fixture"))]
            characterization: configuration.characterization,
        })
    }

    #[cfg(test)]
    pub(crate) fn codex_registration(&self) -> CodexBindingRegistration {
        self.codex.clone()
    }

    #[cfg(test)]
    pub(crate) fn codex_runtime(
        &self,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        session_id: SageAgentSessionId,
    ) -> Result<CodexRuntime, AdapterRuntimeError> {
        let binding = self
            .refresh_codex_binding()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        CodexRuntime::from_admitted_binding(binding, project_root, runtime_epoch, session_id)
            .inspect_err(|_| {
                self.codex.invalidate_drift();
            })
    }

    pub(crate) fn codex_discovery_manager(&self) -> Arc<CodexDiscoveryManager> {
        Arc::clone(&self.codex_discovery)
    }

    pub(crate) fn codex_auth_manager(&self) -> Arc<CodexAuthManager> {
        Arc::clone(&self.codex_auth)
    }

    pub(crate) fn accepts_codex_binding(&self, requested: &BindingId) -> bool {
        self.refresh_codex_binding()
            .is_ok_and(|binding| binding.binding_id() == requested.as_str())
    }

    pub(super) fn refresh_codex_binding(
        &self,
    ) -> Result<
        Arc<crate::acp::bindings::codex::AdmittedCodexBinding>,
        crate::acp::bindings::codex::CodexAdmissionUnavailable,
    > {
        self.codex.refresh().inspect_err(|_| {
            self.codex_discovery.invalidate();
            self.codex_auth.invalidate();
        })
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
        load_chat_page(&self.database.connection(), project_id, chat_id, cursor)
    }

    pub(crate) fn intent_snapshot(
        &self,
        project_id: SageProjectId,
        project_generation: ProjectGeneration,
        chat_id: SageChatId,
    ) -> Result<IntentSnapshot, StoreError> {
        load_intent_snapshot(&self.database, project_id, project_generation, chat_id)
    }

    pub(crate) fn project_switched(
        &self,
        active_project: &SageProjectId,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let discovery = self
            .codex_discovery
            .project_switched(Duration::from_millis(
                crate::acp::limits::LIMITS.deadlines_ms.explicit_close,
            ))
            .map_err(map_discovery_shutdown);
        self.scheduler.project_switched(active_project);
        self.codex_auth.invalidate();
        let runtime = self.runtime.active();
        if let Some(runtime) = runtime {
            let _ = runtime.project_switched(active_project.clone());
        }
        if let Some(notifier) = &self.notifier {
            notifier.set_active_project(active_project.clone());
        }
        discovery
    }

    pub(crate) fn acknowledge_projection_bootstrap(&self) {
        if let Some(notifier) = &self.notifier {
            let _ = notifier.acknowledge_authoritative_bootstrap();
        }
    }

    #[cfg(any(test, feature = "acp-fixture"))]
    #[cfg_attr(not(test), allow(dead_code))]
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
            .field("codex_admitted", &self.codex.is_available())
            .field("notification_active", &self.notifier.is_some())
            .finish_non_exhaustive()
    }
}

fn journal_store_error(error: JournalError) -> StoreError {
    match error {
        JournalError::Store(error) => error,
        JournalError::Capacity { .. } => StoreError::Capacity {
            resource: "recovery queue",
            limit: crate::acp::limits::LIMITS
                .queues
                .journal_control_batches_inside_total,
        },
        JournalError::Deadline => StoreError::Corrupt("recovery journal deadline"),
        JournalError::BatchLimit | JournalError::Closed | JournalError::SequenceExhausted => {
            StoreError::Corrupt("recovery journal")
        }
    }
}

#[cfg(test)]
mod tests;
