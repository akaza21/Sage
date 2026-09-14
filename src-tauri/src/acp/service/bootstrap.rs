use std::{sync::Arc, time::Duration};

use crate::{
    acp::{
        bindings::registry::{AuthProvider, ModelProvider},
        contract::SageProjectId,
        runtime::{
            BoundedThread, JournalError, JournalHandle, JournalLane, ProjectionNotifier,
            ProjectionPublisher, SessionCoordinator,
        },
        scheduler::Scheduler,
        store::{DatabaseAcpStore, StoreError, StoreMutation, StoreMutationResult},
    },
    persistence::Database,
};

use super::{
    AcpService, CodexAuthManager, CodexDiscoveryManager, RuntimeSlot, ServiceConfiguration,
};

impl AcpService {
    pub(super) fn build(
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
        let codex_gate = super::codex_operation::CodexOperationGate::new();
        let record = configuration.registry.codex();
        let codex_discovery = match record.model_provider {
            ModelProvider::CodexAccount => CodexDiscoveryManager::with_gate(
                record.admission().clone(),
                Arc::clone(&codex_gate),
            ),
        };
        let codex_auth = match record.auth_provider {
            AuthProvider::CodexHarness => {
                CodexAuthManager::new(record.admission().clone(), Arc::clone(&codex_gate))
            }
        };
        Ok(Self {
            read_text_files: super::ReadTextFileService::new(database.clone()),
            database,
            scheduler,
            journal,
            journal_thread: std::sync::Mutex::new(Some(journal_thread)),
            sessions,
            runtime: Arc::new(RuntimeSlot::new()),
            publisher,
            notifier,
            page_reads: super::PageReadAdmission::default(),
            registry: configuration.registry,
            codex_gate,
            codex_discovery,
            codex_auth,
            codex_recovery: Arc::new(std::sync::Mutex::new(None)),
            #[cfg(any(debug_assertions, feature = "diagnostics"))]
            codex_runtime_identity: Arc::new(super::CodexRuntimeIdentityLedger::new()),
            #[cfg(test)]
            codex_runtime_release_fail_once: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            codex_runtime_release_fail_always: std::sync::atomic::AtomicBool::new(false),
            #[cfg(all(windows, feature = "acp-fixture"))]
            characterization: configuration.characterization,
            #[cfg(all(test, windows))]
            last_shutdown_stage: std::sync::Mutex::new("none"),
            #[cfg(all(test, windows))]
            last_cancel_stage: std::sync::Mutex::new("none"),
        })
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
