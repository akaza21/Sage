use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use rusqlite::{OptionalExtension, params};

use crate::{
    acp::{
        contract::{
            EntityRevision, ProjectGeneration, RuntimeEpoch, SageChatId, SageProjectId, SageTurnId,
        },
        intent::{AcpChatPage, IntentSnapshot, PageCursor},
        page_admission::{PageAdmissionError, PageReadAdmission, PageReadLease},
        runtime::{
            BoundedThread, JournalError, JournalHandle, JournalLane, NotificationSink,
            ProjectionNotifier, ProjectionPublisher, SessionCoordinator, ThreadWaitError,
        },
        scheduler::Scheduler,
        store::{DatabaseAcpStore, StoreError, StoreMutation, StoreMutationResult, load_chat_page},
    },
    persistence::Database,
};

mod runtime_slot;

use runtime_slot::RuntimeSlot;

pub(crate) struct AcpService {
    database: Database,
    scheduler: Scheduler,
    journal: JournalHandle,
    journal_thread: Mutex<Option<BoundedThread>>,
    sessions: SessionCoordinator,
    runtime: RuntimeSlot,
    notifier: Option<Arc<ProjectionNotifier>>,
    page_reads: PageReadAdmission,
}

impl AcpService {
    #[cfg(test)]
    pub(crate) fn new(database: Database) -> Result<Self, StoreError> {
        Self::build(
            database,
            Arc::new(crate::acp::runtime::ProjectionMailbox::default()),
            None,
        )
    }

    pub(crate) fn new_with_notification_sink(
        database: Database,
        sink: Arc<dyn NotificationSink>,
    ) -> Result<Self, StoreError> {
        let runtime = tauri::async_runtime::handle();
        let notifier = Arc::new(ProjectionNotifier::start_with(sink, move |worker| {
            runtime.inner().spawn(worker)
        }));
        Self::build(database, notifier.clone(), Some(notifier))
    }

    fn build(
        database: Database,
        publisher: Arc<dyn ProjectionPublisher>,
        notifier: Option<Arc<ProjectionNotifier>>,
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
        Ok(Self {
            database,
            scheduler,
            journal,
            journal_thread: Mutex::new(Some(journal_thread)),
            sessions,
            runtime: RuntimeSlot::new(),
            notifier,
            page_reads: PageReadAdmission::default(),
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
        type SnapshotRow = (
            i64,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<i64>,
        );
        let row: SnapshotRow = self
            .database
            .connection()
            .query_row(
                "SELECT c.projection_revision, h.session_id, e.runtime_epoch,
                        t.id, t.state, t.revision
                 FROM chats c
                 LEFT JOIN chat_session_heads h
                   ON h.project_id=c.project_id AND h.chat_id=c.id
                 LEFT JOIN session_runtime_epochs e
                   ON e.project_id=c.project_id AND e.chat_id=c.id
                  AND e.session_id=h.session_id
                  AND e.state IN ('starting','running','terminating')
                 LEFT JOIN turns t
                   ON t.project_id=c.project_id AND t.chat_id=c.id
                  AND t.session_id=h.session_id
                  AND t.state IN (
                    'queued','sending','running','waitingModel','waitingTool',
                    'waitingApproval','waitingUser','cancelling'
                  )
                 WHERE c.project_id=?1 AND c.id=?2",
                params![project_id.as_str(), chat_id.as_str()],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()?
            .ok_or(StoreError::Conflict { resource: "chat" })?;
        Ok(IntentSnapshot {
            project_id,
            project_generation,
            chat_id,
            projection_revision: crate::acp::contract::ProjectionRevision::new(checked_counter(
                row.0,
                "projection revision",
            )?)
            .map_err(|_| StoreError::Corrupt("projection revision"))?,
            session_id: row
                .1
                .map(crate::acp::contract::SageAgentSessionId::parse)
                .transpose()
                .map_err(|_| StoreError::Corrupt("session id"))?,
            runtime_epoch: row
                .2
                .map(|value| {
                    RuntimeEpoch::new(checked_counter(value, "runtime epoch")?)
                        .map_err(|_| StoreError::Corrupt("runtime epoch"))
                })
                .transpose()?,
            turn_id: row
                .3
                .map(SageTurnId::parse)
                .transpose()
                .map_err(|_| StoreError::Corrupt("turn id"))?,
            turn_state: row
                .4
                .as_deref()
                .map(crate::acp::store::parse_turn_state)
                .transpose()?,
            turn_revision: row
                .5
                .map(|value| {
                    EntityRevision::new(checked_counter(value, "turn revision")?)
                        .map_err(|_| StoreError::Corrupt("turn revision"))
                })
                .transpose()?,
        })
    }

    pub(crate) fn project_switched(&self, active_project: &SageProjectId) {
        self.scheduler.project_switched(active_project);
        let runtime = self.runtime.active();
        if let Some(runtime) = runtime {
            let _ = runtime.project_switched(active_project.clone());
        }
        if let Some(notifier) = &self.notifier {
            notifier.set_active_project(active_project.clone());
        }
    }

    pub(crate) fn acknowledge_projection_bootstrap(&self) {
        if let Some(notifier) = &self.notifier {
            let _ = notifier.acknowledge_authoritative_bootstrap();
        }
    }

    pub(crate) fn shutdown_blocking(&self) -> Result<(), crate::acp::runtime::HostError> {
        let started = Instant::now();
        let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        let mut failure = None;
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
            .field("notification_active", &self.notifier.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for AcpService {
    fn drop(&mut self) {
        let started = Instant::now();
        let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
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

fn checked_counter(value: i64, resource: &'static str) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::Corrupt(resource))
}

#[cfg(test)]
mod tests;
