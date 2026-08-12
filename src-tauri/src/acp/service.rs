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
        intent::{AcpChatPage, IntentSnapshot, PageCursor},
        page_admission::{PageAdmissionError, PageReadAdmission, PageReadLease},
        runtime::{
            BoundedThread, JournalError, JournalHandle, JournalLane, ProjectionNotifier,
            ProjectionPublisher, SessionCoordinator, ThreadWaitError,
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
mod codex_discovery;
mod configuration;
mod intent_snapshot;
#[cfg(any(test, feature = "acp-fixture"))]
#[allow(dead_code)] // The packaged reload orchestrator consumes this next.
mod renderer_recreation;
mod runtime_slot;

#[cfg(all(windows, feature = "acp-fixture"))]
pub(crate) use characterization::{
    BINDING_DIGEST_HEX as CHARACTERIZATION_BINDING_DIGEST,
    BINDING_ID as CHARACTERIZATION_BINDING_ID, CharacterizationArtifact, CharacterizationBinding,
    CharacterizationCase, CharacterizationError,
};
use codex_discovery::CodexDiscoveryManager;
use configuration::ServiceConfiguration;
use intent_snapshot::load_intent_snapshot;
#[cfg(any(test, feature = "acp-fixture"))]
use renderer_recreation::RendererRecreationNativeSnapshot;
use runtime_slot::RuntimeSlot;

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
    codex_discovery: Arc<CodexDiscoveryManager>,
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
        let codex_discovery = CodexDiscoveryManager::new(configuration.codex.clone());
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
            codex_discovery,
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
            .codex
            .refresh()
            .map_err(|_| AdapterRuntimeError::LaunchUnavailable)?;
        CodexRuntime::from_admitted_binding(binding, project_root, runtime_epoch, session_id)
            .inspect_err(|_| {
                self.codex.invalidate_drift();
            })
    }

    pub(crate) fn refresh_codex_registration(
        &self,
    ) -> Result<(), crate::acp::bindings::codex::CodexAdmissionUnavailable> {
        self.codex.refresh().map(drop).inspect_err(|_| {
            self.codex_discovery.invalidate();
        })
    }

    pub(crate) fn codex_discovery_manager(&self) -> Arc<CodexDiscoveryManager> {
        Arc::clone(&self.codex_discovery)
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
            .invalidate_and_wait(Duration::from_millis(
                crate::acp::limits::LIMITS.deadlines_ms.explicit_close,
            ))
            .map_err(map_discovery_shutdown);
        self.scheduler.project_switched(active_project);
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

    pub(crate) fn shutdown_blocking(&self) -> Result<(), crate::acp::runtime::HostError> {
        let started = Instant::now();
        let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        let mut failure = None;
        if let Err(error) = self
            .codex_discovery
            .shutdown(budget.saturating_sub(started.elapsed()))
        {
            failure = Some(map_discovery_shutdown(error));
        }
        let mut runtime_stopped = true;
        if let Some(runtime) = self.runtime.begin_shutdown()
            && let Err(error) = runtime.shutdown()
        {
            let _ = self.runtime.shutdown_failed();
            runtime_stopped = false;
            failure = Some(error);
        }
        if let Err(error) = self.stop_notifier(budget.saturating_sub(started.elapsed())) {
            failure.get_or_insert(error);
        }
        self.journal.close_when_drained();
        if let Err(error) = self.wait_journal(budget.saturating_sub(started.elapsed())) {
            self.journal.shutdown();
            failure.get_or_insert(error);
        }
        if runtime_stopped && self.runtime.finish_shutdown().is_err() {
            failure.get_or_insert(crate::acp::runtime::HostError::RecoveryRequired);
        }
        failure.map_or(Ok(()), Err)
    }

    fn stop_notifier(&self, timeout: Duration) -> Result<(), crate::acp::runtime::HostError> {
        let Some(notifier) = self.notifier.as_ref().map(Arc::clone) else {
            return Ok(());
        };
        let clean = Arc::new(AtomicBool::new(false));
        let completed = Arc::clone(&clean);
        let mut thread = BoundedThread::spawn("sage-acp-notifier-stop", move || {
            let stopped = tauri::async_runtime::block_on(notifier.shutdown_with_timeout(timeout));
            completed.store(stopped, Ordering::Release);
        })
        .map_err(|_| crate::acp::runtime::HostError::Unavailable)?;
        thread.wait(timeout).map_err(map_thread_wait)?;
        if clean.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(crate::acp::runtime::HostError::Deadline)
        }
    }

    fn wait_journal(&self, timeout: Duration) -> Result<(), crate::acp::runtime::HostError> {
        let mut thread = self
            .journal_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(worker) = thread.as_mut() else {
            return Ok(());
        };
        match worker.wait(timeout) {
            Ok(()) => {
                *thread = None;
                Ok(())
            }
            Err(error) => Err(map_thread_wait(error)),
        }
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

impl Drop for AcpService {
    fn drop(&mut self) {
        let started = Instant::now();
        let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        let _ = self
            .codex_discovery
            .shutdown(budget.saturating_sub(started.elapsed()));
        let mut runtime_stopped = true;
        if let Some(runtime) = self.runtime.begin_shutdown()
            && runtime.shutdown().is_err()
        {
            let _ = self.runtime.shutdown_failed();
            runtime_stopped = false;
        }
        let _ = self.stop_notifier(budget.saturating_sub(started.elapsed()));
        self.journal.close_when_drained();
        if self
            .wait_journal(budget.saturating_sub(started.elapsed()))
            .is_err()
        {
            self.journal.shutdown();
        }
        if runtime_stopped {
            let _ = self.runtime.finish_shutdown();
        }
    }
}

fn map_thread_wait(error: ThreadWaitError) -> crate::acp::runtime::HostError {
    match error {
        ThreadWaitError::Timeout => crate::acp::runtime::HostError::Deadline,
        ThreadWaitError::Panicked => crate::acp::runtime::HostError::Unavailable,
    }
}

fn map_discovery_shutdown(
    error: crate::acp::bindings::codex::CodexDiscoveryRunError,
) -> crate::acp::runtime::HostError {
    match error {
        crate::acp::bindings::codex::CodexDiscoveryRunError::Timeout => {
            crate::acp::runtime::HostError::Deadline
        }
        crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup => {
            crate::acp::runtime::HostError::RecoveryRequired
        }
        _ => crate::acp::runtime::HostError::Unavailable,
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
