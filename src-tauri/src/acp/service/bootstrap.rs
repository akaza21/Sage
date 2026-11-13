use std::{sync::Arc, time::Duration};

use crate::{
    acp::{
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

use super::{AcpService, CodexAuthManager, CodexDiscoveryManager, ServiceConfiguration};

impl AcpService {
    pub(super) fn build(
        database: Database,
        publisher: Arc<dyn ProjectionPublisher>,
        notifier: Option<Arc<ProjectionNotifier>>,
        configuration: ServiceConfiguration,
    ) -> Result<Self, StoreError> {
        let store = DatabaseAcpStore::new(database.clone());
        let (journal, runner) = JournalHandle::new(store);
        let journal = journal.with_workspace_tracking(database.clone());
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
        {
            let mut connection = database
                .write_connection()
                .map_err(|_| StoreError::Corrupt("browser recovery storage"))?;
            crate::acp::store::browser_operations::recover_started(
                &mut connection,
                crate::persistence::now_millis(),
            )?;
        }
        let session_starts =
            SessionCoordinator::new(journal.clone(), Scheduler::new(), Arc::clone(&publisher));
        let lanes = (0..super::LIVE_RUNTIME_LANES)
            .map(|_| super::live_lanes::LiveLane::new(&journal, &publisher))
            .collect();
        if let Some(notifier) = &notifier
            && let Ok(Some(project_id)) = database.active_project_id()
            && let Ok(project_id) = SageProjectId::parse(project_id)
        {
            notifier.set_active_project(project_id);
        }
        let codex_gate = super::codex_operation::CodexOperationGate::new();
        let codex_discovery = CodexDiscoveryManager::with_gate(
            configuration.registry.codex_admission().clone(),
            Arc::clone(&codex_gate),
        );
        let codex_auth = CodexAuthManager::new(
            configuration.registry.codex_admission().clone(),
            Arc::clone(&codex_gate),
        );
        let service = Self {
            file_stages: Arc::new(std::sync::Mutex::new(Default::default())),
            file_stage_reader: Arc::new(tokio::sync::Mutex::new(())),
            read_text_files: super::ReadTextFileService::new(database.clone()),
            browser_permissions: std::sync::Mutex::new(std::collections::HashMap::new()),
            browser_attachments: crate::acp::browser::attachment::BrowserAttachments::default(),
            #[cfg(windows)]
            browser_app: std::sync::Mutex::new(None),
            database,
            journal,
            journal_thread: std::sync::Mutex::new(Some(journal_thread)),
            session_starts,
            lanes,
            idle_release: Default::default(),
            publisher,
            notifier,
            page_reads: super::PageReadAdmission::default(),
            registry: configuration.registry,
            codex_gate,
            codex_discovery,
            codex_auth,
            #[cfg(any(debug_assertions, feature = "diagnostics"))]
            codex_runtime_identity: Arc::new(super::CodexRuntimeIdentityLedger::new()),
            #[cfg(test)]
            codex_runtime_release_fail_once: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            lane_busy_refusals: Default::default(),
            #[cfg(test)]
            yield_lane_on_busy: Default::default(),
            #[cfg(test)]
            codex_runtime_release_fail_always: std::sync::atomic::AtomicBool::new(false),
            #[cfg(all(windows, feature = "acp-fixture"))]
            characterization: configuration.characterization,
            #[cfg(all(test, windows))]
            last_shutdown_stage: std::sync::Mutex::new("none"),
            #[cfg(all(test, windows))]
            last_cancel_stage: std::sync::Mutex::new("none"),
            #[cfg(test)]
            runtime_port_finish_park_failure: std::sync::Mutex::new(false),
        };
        let recorder = service.session_config_recorder();
        let snapshot = service.session_config_snapshot_recorder();
        for lane in service.lanes() {
            lane.runtime.config.set_recorder(recorder.clone());
            lane.runtime.config.set_snapshot_recorder(snapshot.clone());
        }
        Ok(service)
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
