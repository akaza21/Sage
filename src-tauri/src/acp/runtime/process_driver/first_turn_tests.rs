use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use uuid::Uuid;

use crate::{
    acp::{
        bindings::adapter::{
            AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterIdentity,
            AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
        },
        bindings::codex::{CodexRuntime, Platform},
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RequiredText, RuntimeEpoch,
            SageChatId, SageProjectId, SchemaVersionV1,
        },
        intent::{CapabilitySnapshotV1, ExternalId, IntentSnapshot, Prompt, SafeResourceRef},
        lifecycle::{DeliveryState, TurnState},
        process::{LaunchError, TreeExit},
        runtime::{
            ActivatedSession, ActivationJournal, BoundedThread, CoordinatorActivationJournal,
            HostCancelIntent, HostError, HostPromptIntent, IdleActivationParts, JournalHandle,
            LiveRuntimePort, LiveWrite, ProjectIntentAuthority, ProjectionMailbox, SendCoordinator,
            SessionCoordinator,
        },
        scheduler::{RuntimeOwner, Scheduler},
        store::{
            AcpStore, ActivateSessionCommand, CompleteActivationCommand, DatabaseAcpStore,
            PrepareTurnCommand, SessionMutation, StartSessionCommand, StoreCommit, StoreError,
            StoreMutation,
        },
        transport::{Frame, FrameWriteOutcome, OutboundFrame},
    },
    persistence::Database,
};

#[cfg(feature = "acp-fixture")]
use super::worker_command::WorkerDeadlineAbort;
use super::*;

#[path = "first_turn_tests/steer_cancel_tests.rs"]
mod steer_cancel_tests;

#[path = "first_turn_tests/model_catalog_tests.rs"]
mod model_catalog_tests;

#[path = "first_turn_tests/cancel_shutdown_tests.rs"]
mod cancel_shutdown_tests;

#[path = "first_turn_tests/cancel_handoff_tests.rs"]
mod cancel_handoff_tests;

mod write_observation_tests;

struct Authority(IntentSnapshot);

struct MutableAuthority(Arc<Mutex<IntentSnapshot>>);

struct Adapter {
    cancel_count: Arc<AtomicUsize>,
    final_stdout: Option<AdapterIdentity>,
    config_pending: bool,
}

struct ConfigThenLateAdapter {
    reject_next: bool,
}

struct SequencedAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: crate::acp::contract::SageAgentSessionId,
    external_session_id: ExternalId,
    pending_message: Option<RequiredText<512>>,
    prompt_ids: Arc<Mutex<Vec<String>>>,
    cancel_count: Arc<AtomicUsize>,
    terminal_states: Arc<Mutex<VecDeque<TurnState>>>,
    next_request: u64,
    next_external_turn: u64,
}

impl HarnessAdapter for Adapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        let token = match command {
            AdapterCommand::Prompt {
                client_message_id, ..
            } => {
                if let Some(identity) = self.final_stdout.as_mut() {
                    identity.turn_client_message_id = Some(client_message_id);
                }
                EgressToken::request(1)
            }
            AdapterCommand::Cancel => {
                self.cancel_count.fetch_add(1, Ordering::SeqCst);
                EgressToken::cancel_request(1)
            }
            _ => EgressToken::request(1),
        };
        Ok(AdapterEgress::new(
            OutboundFrame::new(b"{}".to_vec()).unwrap(),
            token,
        ))
    }

    fn ingest(
        &mut self,
        frame: &Frame,
        _: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let method = serde_json::from_slice::<serde_json::Value>(frame.as_bytes())
            .ok()
            .and_then(|value| {
                value
                    .get("method")
                    .and_then(|method| method.as_str())
                    .map(str::to_owned)
            });
        match method.as_deref() {
            Some("mcpServer/startupStatus/updated") => {
                return AdapterBatch::new(vec![AdapterEvent::IgnoredLate], vec![]);
            }
            Some("turn/started") => {
                return AdapterBatch::new(
                    vec![AdapterEvent::Error(
                        crate::acp::intent::NormalizedPayload::Error {
                            kind: None,
                            code: crate::acp::contract::SafeText::new("code", "unexpected")
                                .unwrap(),
                            message: crate::acp::contract::SafeText::new(
                                "message",
                                "unexpected before dispatch",
                            )
                            .unwrap(),
                            retryable: false,
                        },
                    )],
                    vec![],
                );
            }
            _ => {}
        }
        match self.final_stdout.take() {
            Some(id) => AdapterBatch::new(
                vec![
                    AdapterEvent::Turn {
                        id: id.clone(),
                        state: TurnState::Running,
                    },
                    AdapterEvent::Turn {
                        id,
                        state: TurnState::Completed,
                    },
                ],
                vec![],
            ),
            None => Err(AdapterRuntimeError::Operation),
        }
    }

    fn config_pending(&self) -> bool {
        self.config_pending
    }

    fn configure(
        &mut self,
        _: &crate::acp::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.config_pending = true;
        Ok(Some(AdapterEgress::new(
            OutboundFrame::new(b"{}".to_vec()).unwrap(),
            EgressToken::request(1),
        )))
    }

    fn commit_mandatory_egress(&mut self, _: &EgressToken) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }

    fn resolve_egress(
        &mut self,
        _: EgressToken,
        _: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }
}

impl HarnessAdapter for ConfigThenLateAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, _: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        Err(AdapterRuntimeError::Operation)
    }

    fn ingest(
        &mut self,
        _: &Frame,
        _: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        if std::mem::take(&mut self.reject_next) {
            return Err(AdapterRuntimeError::ConfigMutationRejected);
        }
        AdapterBatch::new(vec![AdapterEvent::IgnoredLate], vec![])
    }

    fn commit_mandatory_egress(&mut self, _: &EgressToken) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }

    fn resolve_egress(
        &mut self,
        _: EgressToken,
        _: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }
}

impl HarnessAdapter for SequencedAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        match command {
            AdapterCommand::Prompt {
                client_message_id, ..
            } => {
                self.prompt_ids
                    .lock()
                    .unwrap()
                    .push(client_message_id.as_str().to_owned());
                self.pending_message = Some(client_message_id);
            }
            AdapterCommand::Cancel => {
                self.cancel_count.fetch_add(1, Ordering::SeqCst);
            }
            _ => {}
        }
        self.next_request += 1;
        Ok(AdapterEgress::new(
            OutboundFrame::new(b"{}".to_vec()).unwrap(),
            EgressToken::request(self.next_request),
        ))
    }

    fn ingest(
        &mut self,
        _: &Frame,
        _: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let message = self
            .pending_message
            .take()
            .ok_or(AdapterRuntimeError::Operation)?;
        self.next_external_turn += 1;
        let identity = AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id: self.external_session_id.clone(),
            turn_id: Some(
                ExternalId::new(
                    "externalTurn",
                    format!("external-turn-{}", self.next_external_turn),
                )
                .unwrap(),
            ),
            turn_client_message_id: Some(message),
        };
        let terminal = self
            .terminal_states
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(TurnState::Completed);
        let mut events = Vec::with_capacity(2);
        if terminal != TurnState::Cancelled {
            events.push(AdapterEvent::Turn {
                id: identity.clone(),
                state: TurnState::Running,
            });
        }
        events.push(AdapterEvent::Turn {
            id: identity,
            state: terminal,
        });
        AdapterBatch::new(events, Vec::new())
    }

    fn commit_mandatory_egress(&mut self, _: &EgressToken) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }

    fn resolve_egress(
        &mut self,
        _: EgressToken,
        _: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }
}

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}

struct Transport(Mutex<VecDeque<FrameWriteOutcome>>);

impl crate::acp::runtime::EgressTransport for Transport {
    fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(FrameWriteOutcome::Complete)
    }
}

impl StdinTransport for Transport {
    fn close(&mut self) {}
}

/// This test control never launches or owns an OS child. Its zero check proves
/// only that the driver crossed the required no-child-process boundary.
struct NoChildProcessControl {
    zero_checks: Arc<AtomicUsize>,
}

struct CommitUnknownOnceStore {
    inner: DatabaseAcpStore,
    begin_send: Arc<Mutex<Option<bool>>>,
    adapter_inbound_batch: Arc<Mutex<Option<bool>>>,
    active_turn_lookups: Arc<AtomicUsize>,
}

impl AcpStore for CommitUnknownOnceStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        if matches!(batch, [StoreMutation::LookupActiveTurn(_)]) {
            self.active_turn_lookups.fetch_add(1, Ordering::SeqCst);
        }
        let injection = if matches!(batch, [StoreMutation::BeginSend(_)]) {
            self.begin_send.lock().unwrap().take()
        } else if matches!(batch, [StoreMutation::AdapterInboundBatch(_)]) {
            self.adapter_inbound_batch.lock().unwrap().take()
        } else {
            None
        };
        match injection {
            Some(true) => {
                self.inner.commit(batch)?;
                Err(StoreError::CommitUnknown)
            }
            Some(false) => Err(StoreError::CommitUnknown),
            None => self.inner.commit(batch),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloseBeforeWriteCase {
    EarlyStdoutBeforeObservation,
    Idle,
    IdleReaderEofPrompt,
    IdleReaderEofClose,
    IdleReaderEofConfigure,
    IdleReaderEofThenProcessExitDeadline,
    IdleReaderEofDeadline,
    IdlePendingConfigReaderEofDeadline,
    PreparedReaderEofDispatch,
    PreparedEarlyStdout,
    PreparedEarlyFailedStdout,
    PreparedUnexpectedStdout,
    ActiveProcessExitDeadline,
    ActiveReadyForWriteProcessExitDeadline,
    ActiveReaderEofThenProcessExitDeadline,
    ReusableReaderEofThenProcessExitDeadline,
    ReusableReaderEofDeadline,
    ReusablePendingConfigReaderEofDeadline,
    Prepared,
    BeginSendAppliedUnknown,
    BeginSendNotAppliedUnknown,
    FinalStdoutAppliedUnknown,
    FinalStdoutNotAppliedUnknown,
}

impl CloseBeforeWriteCase {
    fn prepares_turn(self) -> bool {
        !matches!(
            self,
            Self::Idle
                | Self::IdleReaderEofPrompt
                | Self::IdleReaderEofClose
                | Self::IdleReaderEofConfigure
                | Self::IdleReaderEofThenProcessExitDeadline
                | Self::IdleReaderEofDeadline
                | Self::IdlePendingConfigReaderEofDeadline
        )
    }

    fn begin_send_injection(self) -> Option<bool> {
        match self {
            Self::BeginSendAppliedUnknown => Some(true),
            Self::BeginSendNotAppliedUnknown => Some(false),
            Self::EarlyStdoutBeforeObservation
            | Self::Idle
            | Self::IdleReaderEofPrompt
            | Self::IdleReaderEofClose
            | Self::IdleReaderEofConfigure
            | Self::IdleReaderEofThenProcessExitDeadline
            | Self::IdleReaderEofDeadline
            | Self::IdlePendingConfigReaderEofDeadline
            | Self::PreparedReaderEofDispatch
            | Self::PreparedEarlyStdout
            | Self::PreparedEarlyFailedStdout
            | Self::PreparedUnexpectedStdout
            | Self::ActiveProcessExitDeadline
            | Self::ActiveReadyForWriteProcessExitDeadline
            | Self::ActiveReaderEofThenProcessExitDeadline
            | Self::ReusableReaderEofThenProcessExitDeadline
            | Self::ReusableReaderEofDeadline
            | Self::ReusablePendingConfigReaderEofDeadline
            | Self::Prepared
            | Self::FinalStdoutAppliedUnknown
            | Self::FinalStdoutNotAppliedUnknown => None,
        }
    }

    fn final_stdout_injection(self) -> Option<bool> {
        match self {
            Self::FinalStdoutAppliedUnknown => Some(true),
            Self::FinalStdoutNotAppliedUnknown => Some(false),
            _ => None,
        }
    }

    fn dispatches_prompt(self) -> bool {
        self == Self::EarlyStdoutBeforeObservation
            || self == Self::ActiveProcessExitDeadline
            || self == Self::ActiveReadyForWriteProcessExitDeadline
            || self == Self::ActiveReaderEofThenProcessExitDeadline
            || self == Self::ReusableReaderEofThenProcessExitDeadline
            || self == Self::ReusableReaderEofDeadline
            || self == Self::ReusablePendingConfigReaderEofDeadline
            || self == Self::PreparedReaderEofDispatch
            || self.begin_send_injection().is_some()
            || self.final_stdout_injection().is_some()
    }
}

#[derive(Default)]
struct DeadlineClock {
    now: AtomicI64,
    advance_after_next_read: Mutex<Option<(Option<Arc<DriverMailbox>>, i64)>>,
    advance_after_read: Mutex<Option<(usize, i64)>>,
    reader_eof_after_read: Mutex<Option<(usize, Arc<DriverMailbox>)>>,
    read_count: AtomicUsize,
    hook_read: AtomicUsize,
    max_reads: AtomicUsize,
}

impl DeadlineClock {
    fn set(&self, value: i64) {
        self.now.store(value, Ordering::SeqCst);
    }

    fn advance_after_next_read_with_process_exit(&self, mailbox: Arc<DriverMailbox>, value: i64) {
        *self.advance_after_next_read.lock().unwrap() = Some((Some(mailbox), value));
    }

    fn advance_after_next_read(&self, value: i64) {
        *self.advance_after_next_read.lock().unwrap() = Some((None, value));
    }

    fn advance_after_read(&self, read: usize, value: i64) {
        *self.advance_after_read.lock().unwrap() = Some((read, value));
    }

    fn reader_eof_after_read(&self, read: usize, mailbox: Arc<DriverMailbox>) {
        *self.reader_eof_after_read.lock().unwrap() = Some((read, mailbox));
    }

    fn fail_after_reads(&self, value: usize) {
        self.max_reads.store(value, Ordering::SeqCst);
    }

    fn hook_read(&self) -> usize {
        self.hook_read.load(Ordering::SeqCst)
    }

    fn read_count(&self) -> usize {
        self.read_count.load(Ordering::SeqCst)
    }
}

impl DriverClock for DeadlineClock {
    fn now_millis(&self) -> i64 {
        let read = self.read_count.fetch_add(1, Ordering::SeqCst) + 1;
        let max_reads = self.max_reads.load(Ordering::SeqCst);
        assert!(
            max_reads == 0 || read <= max_reads,
            "clock read limit exceeded"
        );
        let now = self.now.load(Ordering::SeqCst);
        let reader_eof = self
            .reader_eof_after_read
            .lock()
            .unwrap()
            .take_if(|(target, _)| *target == read)
            .map(|(_, mailbox)| mailbox);
        if let Some(mailbox) = reader_eof {
            mailbox.signal_reader_terminal(ReaderTerminal::Eof).unwrap();
        }
        if let Some((mailbox, next)) = self.advance_after_next_read.lock().unwrap().take() {
            if let Some(mailbox) = mailbox {
                mailbox
                    .signal_process_exit(TreeExit::Exited { code: Some(17) })
                    .unwrap();
            }
            self.set(next);
            self.hook_read.store(read, Ordering::SeqCst);
        }
        let next = self
            .advance_after_read
            .lock()
            .unwrap()
            .take_if(|(target, _)| *target == read)
            .map(|(_, next)| next);
        if let Some(next) = next {
            self.set(next);
            self.hook_read.store(read, Ordering::SeqCst);
        }
        now
    }
}

fn configured_runner_reaches_reader_eof_deadline(
    state: FirstTurnWorkerState<Adapter, Mapper, Transport, NoChildProcessControl>,
    context: FirstTurnWorkerContext<Authority>,
    mailbox: Arc<DriverMailbox>,
    owner: RuntimeOwner,
    clock: Arc<DeadlineClock>,
    eof_deadline: i64,
) -> TreeExit {
    let completion_observation = Arc::new(Mutex::new(None));
    let observed_completion = Arc::clone(&completion_observation);
    let response = mailbox
        .submit_worker(WorkerCommand::ConfigureSession {
            intent: crate::acp::session_config::ConfigIntent {
                owner,
                expected_config_revision: crate::acp::session_config::ConfigRevision::first(),
                mutation: crate::acp::session_config::ConfigMutation::Mode(
                    crate::acp::session_config::ConfigId::new("mode", "code").unwrap(),
                ),
            },
            completion: crate::acp::runtime::ConfigCompletionSink::new(move |result| {
                *observed_completion.lock().unwrap() = Some(result);
            }),
        })
        .unwrap();
    let reads_before_runner = clock.read_count();
    clock.reader_eof_after_read(reads_before_runner + 1, Arc::clone(&mailbox));
    clock.advance_after_read(reads_before_runner + 2, eof_deadline);
    clock.fail_after_reads(reads_before_runner + 4);
    let runner = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move { super::live_runner::drive_for_test(state, &context).await })
    });
    match response.recv_timeout(tree_termination_deadline()) {
        Ok(Ok(WorkerReply::Unit)) => {}
        Ok(Err(error)) => panic!("configuration reply: {error:?}"),
        Ok(Ok(_)) => panic!("unexpected configuration reply"),
        Err(error) => panic!("configuration response: {error:?}"),
    }
    let (_, reason) = runner.join().unwrap();
    assert_eq!(
        completion_observation.lock().unwrap().take(),
        Some(Err(HostError::RecoveryRequired))
    );
    assert_eq!(clock.hook_read(), reads_before_runner + 2);
    assert!(clock.read_count() <= reads_before_runner + 4);
    reason
}

impl ProcessTreeControl for NoChildProcessControl {
    type ZeroProof = ();

    fn request_termination(&self, _: TreeExit) -> Result<(), LaunchError> {
        Ok(())
    }

    fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    fn wait_zero(&self) -> Result<(TreeExit, Self::ZeroProof), LaunchError> {
        self.zero_checks.fetch_add(1, Ordering::SeqCst);
        Ok((TreeExit::Cancelled, ()))
    }
}

impl ProjectIntentAuthority for Authority {
    fn current_snapshot(&self, _: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.clone())
    }
}

impl ProjectIntentAuthority for MutableAuthority {
    fn current_snapshot(&self, _: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.lock().unwrap().clone())
    }
}

struct FirstTurnFixture {
    database: Database,
    project_id: SageProjectId,
    chat_id: SageChatId,
    now_millis: i64,
    publisher: Arc<ProjectionMailbox>,
    journal: JournalHandle,
    journal_task: JournalThreadCleanup,
    scheduler: Scheduler,
    sessions: SessionCoordinator,
    started: SessionMutation,
    epoch: RuntimeEpoch,
    activation_journal: CoordinatorActivationJournal,
    capabilities: CapabilitySnapshotV1,
    external_session: ExternalId,
    owner: RuntimeOwner,
    ready_projection_revision: ProjectionRevision,
    root: TempRoot,
}

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(prefix: &str) -> Self {
        Self(std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4())))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct JournalThreadCleanup {
    journal: JournalHandle,
    thread: Option<BoundedThread>,
}

impl JournalThreadCleanup {
    fn finish(self) {
        let deadline = Instant::now()
            + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.wal_convergence);
        self.finish_before(deadline);
    }

    fn finish_before(mut self, deadline: Instant) {
        self.journal.close_when_drained();
        let wait_limit =
            Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.wal_convergence);
        let remaining = remaining_before(deadline).min(wait_limit);
        let result = self
            .thread
            .as_mut()
            .expect("journal thread retained")
            .wait_and_join(remaining);
        if result.is_ok() {
            self.thread.take();
        }
        result.unwrap();
    }
}

impl Drop for JournalThreadCleanup {
    fn drop(&mut self) {
        self.journal.shutdown();
        if let Some(thread) = &mut self.thread {
            let _ = thread.wait(tree_termination_deadline());
        }
    }
}

#[cfg(feature = "acp-fixture")]
struct TestThreadCleanup {
    thread: Option<BoundedThread>,
    release: Option<mpsc::SyncSender<()>>,
    mailbox: Arc<DriverMailbox>,
}

#[cfg(feature = "acp-fixture")]
impl TestThreadCleanup {
    fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }

    fn wait(&mut self) {
        self.release();
        let mut thread = self.thread.take().expect("test thread retained");
        thread.wait(tree_termination_deadline()).unwrap();
    }
}

#[cfg(feature = "acp-fixture")]
impl Drop for TestThreadCleanup {
    fn drop(&mut self) {
        self.mailbox.close();
        self.release();
        if let Some(thread) = &mut self.thread {
            let _ = thread.wait(tree_termination_deadline());
        }
    }
}

fn tree_termination_deadline() -> Duration {
    Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.tree_termination)
}

fn mailbox_deadline(clock: &dyn DriverClock, timeout_millis: u64) -> i64 {
    clock
        .now_millis()
        .saturating_add(i64::try_from(timeout_millis).unwrap_or(i64::MAX))
}

fn remaining_before(deadline: Instant) -> Duration {
    deadline
        .checked_duration_since(Instant::now())
        .expect("recovery test exceeded its whole-test deadline")
}

fn mailbox_deadline_before(clock: &dyn DriverClock, deadline: Instant) -> i64 {
    let timeout_millis = u64::try_from(remaining_before(deadline).as_millis()).unwrap_or(u64::MAX);
    mailbox_deadline(clock, timeout_millis)
}

#[cfg(feature = "acp-fixture")]
fn wait_for_queued_control(mailbox: &DriverMailbox) {
    let deadline = Instant::now()
        + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.cancel_dispatch);
    while mailbox.retained_usage().1.0 == 0 {
        assert!(
            Instant::now() < deadline,
            "cancel control was not queued by its deadline"
        );
        std::thread::yield_now();
    }
}

async fn prepare_first_turn_fixture() -> FirstTurnFixture {
    let root = TempRoot::new("sage-first-turn");
    fs::create_dir_all(root.path()).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(root.path()).unwrap();
    let project_id = SageProjectId::parse(project.id).unwrap();
    let chat_id = SageChatId::parse("chat-first-turn").unwrap();
    let now_millis = crate::persistence::now_millis();
    database
        .connection()
        .execute(
            "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
             VALUES (?1,?2,1,'First Turn',0,?3,?3)",
            rusqlite::params![chat_id.as_str(), project_id.as_str(), now_millis],
        )
        .unwrap();
    let publisher = Arc::new(ProjectionMailbox::default());
    let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
    let journal_thread = BoundedThread::spawn("first-turn-journal", move || runner.run_blocking())
        .expect("spawn first-turn journal");
    let journal_task = JournalThreadCleanup {
        journal: journal.clone(),
        thread: Some(journal_thread),
    };
    let scheduler = Scheduler::new();
    let sessions = SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
    let initial = crate::acp::store::load_chat_page(
        &database.connection(),
        project_id.clone(),
        chat_id.clone(),
        None,
    )
    .unwrap();
    let binding = "11".repeat(32);
    let artifact = "22".repeat(32);
    let started = sessions
        .start_session(StartSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: initial.projection_revision,
            client_intent_id: ClientIntentId::parse("intent-start").unwrap(),
            binding_id: RequiredText::new("binding", "binding-test").unwrap(),
            binding_digest: binding.clone(),
            harness_version: None,
            model_id: None,
            now_millis,
        })
        .await
        .unwrap();
    let epoch = RuntimeEpoch::new(1).unwrap();
    let lease = sessions
        .activate_session(ActivateSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::parse("intent-activate").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            artifact_digest: artifact.clone(),
            now_millis: now_millis + 1,
        })
        .await
        .unwrap();
    let capabilities = capabilities();
    let external_session = ExternalId::new("external", "external-session").unwrap();
    let mut activation_journal = CoordinatorActivationJournal::new(lease);
    let ready = activation_journal
        .commit_ready(CompleteActivationCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            binding_digest: binding,
            artifact_digest: artifact,
            external_session_id: external_session.clone(),
            capabilities_json: serde_json::to_string(&capabilities).unwrap(),
            now_millis: now_millis + 2,
        })
        .await
        .unwrap();
    let owner = RuntimeOwner {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: started.session_id.clone(),
        runtime_epoch: epoch,
    };
    FirstTurnFixture {
        database,
        project_id,
        chat_id,
        now_millis,
        publisher,
        journal,
        journal_task,
        scheduler,
        sessions,
        started,
        epoch,
        activation_journal,
        capabilities,
        external_session,
        owner,
        ready_projection_revision: ready.projection_revision,
        root,
    }
}

#[cfg(feature = "acp-fixture")]
pub(crate) struct CancelDispatchObservation {
    pub(crate) cancel_dispatch_ms_maximum: u64,
    pub(crate) control_capacity_borrowed: bool,
}

struct CancelTransport {
    writes: Arc<AtomicUsize>,
}

impl crate::acp::runtime::EgressTransport for CancelTransport {
    fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
        self.writes.fetch_add(1, Ordering::SeqCst);
        FrameWriteOutcome::Complete
    }
}

impl StdinTransport for CancelTransport {
    fn close(&mut self) {}
}

#[cfg(feature = "acp-fixture")]
#[derive(Debug, PartialEq, Eq)]
struct CancelSnapshot {
    state: String,
    delivery_state: String,
    turn_revision: i64,
    session_projection_revision: i64,
    chat_projection_revision: i64,
    cancelling_facts: i64,
    cancel_intent_facts: i64,
}

#[cfg(feature = "acp-fixture")]
fn cancel_snapshot(
    database: &Database,
    turn_id: &crate::acp::contract::SageTurnId,
) -> CancelSnapshot {
    database
        .connection()
        .query_row(
            "SELECT t.state,t.delivery_state,t.revision,s.projection_revision,c.projection_revision,
                    (SELECT COUNT(*) FROM session_facts WHERE turn_id=t.id AND kind='turnCancelling'),
                    (SELECT COUNT(*) FROM session_facts WHERE turn_id=t.id AND kind='turnCancelIntent')
             FROM turns t
             JOIN agent_sessions s ON s.id=t.session_id
             JOIN chats c ON c.id=t.chat_id AND c.project_id=t.project_id
             WHERE t.id=?1",
            [turn_id.as_str()],
            |row| {
                Ok(CancelSnapshot {
                    state: row.get(0)?,
                    delivery_state: row.get(1)?,
                    turn_revision: row.get(2)?,
                    session_projection_revision: row.get(3)?,
                    chat_projection_revision: row.get(4)?,
                    cancelling_facts: row.get(5)?,
                    cancel_intent_facts: row.get(6)?,
                })
            },
        )
        .unwrap()
}

#[cfg(feature = "acp-fixture")]
pub(crate) fn observe_process_live_port_cancel_at_capacity(
    messages: u64,
    bytes: u64,
    deadline_ms: u64,
) -> CancelDispatchObservation {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async move {
            let FirstTurnFixture {
                database,
                project_id,
                chat_id,
                now_millis,
                publisher,
                journal,
                journal_task,
                scheduler,
                sessions: _sessions,
                started,
                epoch,
                activation_journal,
                capabilities,
                external_session,
                owner,
                ready_projection_revision,
                root,
            } = prepare_first_turn_fixture().await;
            let cancel_count = Arc::new(AtomicUsize::new(0));
            let writes = Arc::new(AtomicUsize::new(0));
            let zero_checks = Arc::new(AtomicUsize::new(0));
            let completed = CompletedActivationDriverParts {
                parts: IdleActivationParts::new(
                    Adapter {
                        cancel_count: Arc::clone(&cancel_count),
                        final_stdout: None,
                        config_pending: false,
                    },
                    activation_journal,
                    Mapper,
                    ActivatedSession {
                        binding_digest: [0x11; 32],
                        project_id: project_id.clone(),
                        chat_id: chat_id.clone(),
                        session_id: started.session_id.clone(),
                        external_session_id: external_session,
                        runtime_epoch: epoch,
                        capabilities,
                        models: Vec::new(),
                        next_model_cursor: None,
                    },
                ),
                idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
                transport: CancelTransport {
                    writes: Arc::clone(&writes),
                },
                control: NoChildProcessControl {
                    zero_checks: Arc::clone(&zero_checks),
                },
                owner: owner.clone(),
                clock: Arc::new(SystemDriverClock) as Arc<dyn DriverClock>,
            };
            let authority = Arc::new(Authority(IntentSnapshot {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                projection_revision: ready_projection_revision,
                session_id: Some(started.session_id.clone()),
                runtime_epoch: Some(epoch),
                turn_id: None,
                turn_state: None,
                turn_revision: None,
            }));
            let mailbox = Arc::new(DriverMailbox::new().unwrap());
            let context = FirstTurnWorkerContext {
                owner: owner.clone(),
                coordinator: SendCoordinator::new(authority, journal.clone(), publisher.clone()),
                journal: journal.clone(),
                publisher,
                clock: Arc::new(SystemDriverClock) as Arc<dyn DriverClock>,
                mailbox: Arc::clone(&mailbox),
                session_recovery: None,
                zero_sequence: std::sync::atomic::AtomicU64::new(0),
            };
            let caller_mailbox = Arc::clone(&mailbox);
            let caller_owner = owner.clone();
            let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
            let (go_sender, go_receiver) = mpsc::sync_channel(1);
            let (caller_result_sender, caller_result_receiver) = mpsc::sync_channel(1);
            let caller = BoundedThread::spawn("saturated-cancel-caller", move || {
                let mut port = ProcessLivePort::new(
                    caller_owner.clone(),
                    caller_mailbox,
                    Arc::new(|_| panic!("successful cancel dispatch must not abort the process")),
                );
                let turn_id = port
                    .prepare_prompt(&HostPromptIntent {
                        command: PrepareTurnCommand {
                            project_id: caller_owner.project_id.clone(),
                            project_generation: ProjectGeneration::ZERO,
                            chat_id: caller_owner.chat_id.clone(),
                            session_id: caller_owner.session_id.clone(),
                            runtime_epoch: caller_owner.runtime_epoch,
                            expected_projection_revision: ready_projection_revision,
                            client_intent_id: ClientIntentId::parse("intent-saturated-turn")
                                .unwrap(),
                            prompt: Prompt::new("prompt", "saturated cancel").unwrap(),
                            now_millis: now_millis + 3,
                        },
                        access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
                    })
                    .unwrap();
                assert_eq!(
                    port.dispatch_prompt(&caller_owner, &turn_id),
                    Ok(LiveWrite::Complete)
                );
                port.observe_send(
                    &caller_owner,
                    &turn_id,
                    crate::acp::store::SendObservation::CompleteBytes,
                )
                .unwrap();
                let cancel = HostCancelIntent {
                    owner: caller_owner,
                    turn_id: turn_id.clone(),
                    client_intent_id: ClientIntentId::parse("intent-saturated-cancel").unwrap(),
                    now_millis: now_millis + 4,
                };
                port.begin_cancel(&cancel).unwrap();
                ready_sender.send((turn_id, cancel.clone())).unwrap();
                go_receiver
                    .recv_timeout(Duration::from_millis(
                        crate::acp::limits::LIMITS.deadlines_ms.explicit_close,
                    ))
                    .expect("cancel dispatch release before close deadline");
                let started = Instant::now();
                let result = port.dispatch_cancel(&cancel);
                caller_result_sender
                    .send((result, started.elapsed()))
                    .unwrap();
            })
            .unwrap();
            let mut caller = TestThreadCleanup {
                thread: Some(caller),
                release: Some(go_sender),
                mailbox: Arc::clone(&mailbox),
            };

            let mut state = FirstTurnWorkerState::Idle(completed);
            for timeout_millis in [
                crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc,
                crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc,
                crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc,
                crate::acp::limits::LIMITS.deadlines_ms.explicit_close,
            ] {
                let next = mailbox
                    .wait_next_until(
                        Some(mailbox_deadline(context.clock.as_ref(), timeout_millis)),
                        context.clock.as_ref(),
                    )
                    .unwrap()
                    .expect("first-turn worker command");
                let ControlMessage::Worker(envelope) = next else {
                    panic!("expected first-turn worker command")
                };
                state = state.handle(&context, envelope).await;
            }
            let (turn_id, cancel) = ready_receiver
                .recv_timeout(Duration::from_millis(
                    crate::acp::limits::LIMITS.deadlines_ms.explicit_close,
                ))
                .expect("cancel intent before close deadline");
            let durable_before = cancel_snapshot(&database, &turn_id);
            assert_eq!(
                durable_before,
                CancelSnapshot {
                    state: "cancelling".into(),
                    delivery_state: "completeBytes".into(),
                    turn_revision: 3,
                    session_projection_revision: 7,
                    chat_projection_revision: 7,
                    cancelling_facts: 1,
                    cancel_intent_facts: 1,
                }
            );
            assert_eq!(
                messages,
                crate::acp::limits::LIMITS.queues.driver_data_messages
            );
            assert_eq!(bytes, messages);
            for _ in 0..messages {
                mailbox.try_push_chunk(vec![b'x']).unwrap();
            }
            let saturated_before =
                matches!(mailbox.try_push_chunk(vec![b'x']), Err(MailboxError::Full));
            assert_eq!(
                mailbox.retained_usage().0,
                (messages as usize, bytes as usize)
            );
            caller.release();
            wait_for_queued_control(mailbox.as_ref());

            let FirstTurnWorkerState::Active(mut active) = state else {
                panic!("cancel dispatch requires active first-turn state")
            };
            let step = active
                .turn
                .driver
                .run_next_step(mailbox.as_ref())
                .await
                .unwrap();
            let DriverStep::YieldWorker { envelope, .. } = step else {
                panic!("reserved cancel must yield the live worker")
            };
            assert!(matches!(
                &envelope.command,
                WorkerCommand::DispatchCancel(intent) if intent == &cancel
            ));
            state = FirstTurnWorkerState::Active(active)
                .handle(&context, envelope)
                .await;
            // The caller measures its own elapsed time; this wait only guards
            // against a hang, so scheduling on a loaded machine cannot fail it.
            let (dispatch, elapsed) = caller_result_receiver
                .recv_timeout(crate::test_wait::BUDGET)
                .expect("cancel dispatch returned");
            caller.wait();
            assert_eq!(dispatch, Ok(()));
            assert!(elapsed <= Duration::from_millis(deadline_ms));
            assert!(matches!(state, FirstTurnWorkerState::Active(_)));
            assert_eq!(cancel_count.load(Ordering::SeqCst), 1);
            assert_eq!(writes.load(Ordering::SeqCst), 2);
            assert_eq!(zero_checks.load(Ordering::SeqCst), 0);
            assert_eq!(mailbox.retained_usage(), ((256, 256), (0, 0)));
            let saturated_after =
                matches!(mailbox.try_push_chunk(vec![b'x']), Err(MailboxError::Full));
            assert_eq!(cancel_snapshot(&database, &turn_id), durable_before);

            let exits = Arc::new(Mutex::new(Vec::new()));
            let recorded = Arc::clone(&exits);
            let timeout_mailbox = Arc::clone(&mailbox);
            let timeout_owner = owner.clone();
            let timeout_cancel = cancel.clone();
            let (timeout_result_sender, timeout_result_receiver) = mpsc::sync_channel(1);
            let timeout = BoundedThread::spawn("saturated-cancel-timeout", move || {
                let abort: Arc<dyn WorkerDeadlineAbort> = Arc::new(move |reason| {
                    recorded.lock().unwrap().push(reason);
                });
                let mut port =
                    ProcessLivePort::new_with_timeout(timeout_owner, timeout_mailbox, abort, 50);
                let started = Instant::now();
                timeout_result_sender
                    .send((port.dispatch_cancel(&timeout_cancel), started.elapsed()))
                    .unwrap();
            })
            .unwrap();
            let mut timeout = TestThreadCleanup {
                thread: Some(timeout),
                release: None,
                mailbox: Arc::clone(&mailbox),
            };
            wait_for_queued_control(mailbox.as_ref());
            let queued = mailbox.submit_worker(WorkerCommand::Shutdown).unwrap();
            let (timeout_result, timeout_elapsed) = timeout_result_receiver
                .recv_timeout(crate::test_wait::BUDGET)
                .expect("timed-out cancel dispatch returned");
            timeout.wait();
            assert_eq!(timeout_result, Err(HostError::Deadline));
            assert!(timeout_elapsed < Duration::from_secs(1));
            assert!(matches!(
                queued.recv_timeout(Duration::from_secs(1)),
                Ok(Err(HostError::Closed))
            ));
            assert!(matches!(
                mailbox.try_push_chunk(vec![b'x']),
                Err(MailboxError::Closed)
            ));
            assert_eq!(*exits.lock().unwrap(), [TreeExit::Deadline]);
            assert_eq!(cancel_snapshot(&database, &turn_id), durable_before);
            assert_eq!(writes.load(Ordering::SeqCst), 2);

            drop(state);
            drop(context);
            drop(scheduler);
            drop(journal);
            drop(database);
            journal_task.finish();
            drop(root);

            CancelDispatchObservation {
                cancel_dispatch_ms_maximum: deadline_ms,
                control_capacity_borrowed: !(saturated_before && saturated_after),
            }
        })
}

#[test]
fn first_turn_retry_cancel_and_close_recovery_keep_one_exact_identity() {
    let whole_test_deadline = Instant::now() + Duration::from_secs(60);
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(
                remaining_before(whole_test_deadline),
                first_turn_retry_cancel_and_close_recovery_scenario(whole_test_deadline),
            )
            .await
            .expect("recovery scenario exceeded its whole-test deadline");
        });
}

async fn first_turn_retry_cancel_and_close_recovery_scenario(whole_test_deadline: Instant) {
    let FirstTurnFixture {
        database,
        project_id,
        chat_id,
        now_millis: now,
        publisher,
        journal,
        journal_task,
        scheduler,
        sessions: _sessions,
        started,
        epoch,
        activation_journal,
        capabilities,
        external_session,
        owner,
        ready_projection_revision,
        root,
    } = prepare_first_turn_fixture().await;
    let database_ownership = database.connection_ownership_probe();
    let clock: Arc<dyn DriverClock> = Arc::new(SystemDriverClock);
    let zero_checks = Arc::new(AtomicUsize::new(0));
    let cancel_count = Arc::new(AtomicUsize::new(0));
    let completed = CompletedActivationDriverParts {
        parts: IdleActivationParts::new(
            Adapter {
                cancel_count: Arc::clone(&cancel_count),
                final_stdout: None,
                config_pending: false,
            },
            activation_journal,
            Mapper,
            ActivatedSession {
                binding_digest: [0x11; 32],
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                external_session_id: external_session,
                runtime_epoch: epoch,
                capabilities,
                models: Vec::new(),
                next_model_cursor: None,
            },
        ),
        idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
        transport: Transport(Mutex::new(VecDeque::from([
            FrameWriteOutcome::ZeroByteFailure,
            FrameWriteOutcome::Complete,
            FrameWriteOutcome::DeliveryUnknown { bytes_written: 1 },
        ]))),
        control: NoChildProcessControl {
            zero_checks: Arc::clone(&zero_checks),
        },
        owner: owner.clone(),
        clock: Arc::clone(&clock),
    };
    let authority = Arc::new(Authority(IntentSnapshot {
        project_id: project_id.clone(),
        project_generation: ProjectGeneration::ZERO,
        chat_id: chat_id.clone(),
        projection_revision: ready_projection_revision,
        session_id: Some(started.session_id.clone()),
        runtime_epoch: Some(epoch),
        turn_id: None,
        turn_state: None,
        turn_revision: None,
    }));
    let sends = SendCoordinator::new(authority, journal.clone(), publisher.clone());
    let mailbox = Arc::new(DriverMailbox::new().unwrap());
    let context = FirstTurnWorkerContext {
        owner: owner.clone(),
        coordinator: sends,
        journal: journal.clone(),
        publisher: publisher.clone(),
        clock,
        mailbox: Arc::clone(&mailbox),
        session_recovery: None,
        zero_sequence: std::sync::atomic::AtomicU64::new(0),
    };
    let caller_mailbox = Arc::clone(&mailbox);
    let caller_owner = owner.clone();
    let (caller_finished, caller_completion) = mpsc::sync_channel(1);
    let caller = std::thread::Builder::new()
        .name("first-turn-recovery-caller".into())
        .spawn(move || {
            let mut port = ProcessLivePort::new_with_timeout(
                caller_owner.clone(),
                caller_mailbox,
                Arc::new(|_| {}),
                1_000,
            );
            let turn_id = port
                .prepare_prompt(&HostPromptIntent {
                    command: PrepareTurnCommand {
                        project_id: caller_owner.project_id.clone(),
                        project_generation: ProjectGeneration::ZERO,
                        chat_id: caller_owner.chat_id.clone(),
                        session_id: caller_owner.session_id.clone(),
                        runtime_epoch: caller_owner.runtime_epoch,
                        expected_projection_revision: ready_projection_revision,
                        client_intent_id: ClientIntentId::parse("intent-turn").unwrap(),
                        prompt: Prompt::new("prompt", "hello").unwrap(),
                        now_millis: now + 3,
                    },
                    access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
                })
                .unwrap();
            assert_eq!(
                port.dispatch_prompt(&caller_owner, &turn_id),
                Err(HostError::RecoveryRequired)
            );
            let first = port.dispatch_prompt(&caller_owner, &turn_id).unwrap();
            port.observe_send(
                &caller_owner,
                &turn_id,
                crate::acp::store::SendObservation::ZeroBytes,
            )
            .unwrap();
            let retry = port.dispatch_prompt(&caller_owner, &turn_id).unwrap();
            assert_eq!(
                port.observe_send(
                    &caller_owner,
                    &turn_id,
                    crate::acp::store::SendObservation::CompleteBytes,
                ),
                Err(HostError::RecoveryRequired)
            );
            port.observe_send(
                &caller_owner,
                &turn_id,
                crate::acp::store::SendObservation::CompleteBytes,
            )
            .unwrap();
            let cancel = HostCancelIntent {
                owner: caller_owner.clone(),
                turn_id: turn_id.clone(),
                client_intent_id: ClientIntentId::parse("intent-cancel-turn").unwrap(),
                now_millis: now + 4,
            };
            assert_eq!(port.begin_cancel(&cancel), Err(HostError::RecoveryRequired));
            port.begin_cancel(&cancel).unwrap();
            assert_eq!(
                port.dispatch_cancel(&cancel),
                Err(HostError::RecoveryRequired)
            );
            let close = crate::acp::store::CloseSessionCommand {
                project_id: caller_owner.project_id.clone(),
                chat_id: caller_owner.chat_id.clone(),
                expected_projection_revision: ProjectionRevision::new(9).unwrap(),
                client_intent_id: ClientIntentId::parse("intent-close-turn").unwrap(),
                session_id: caller_owner.session_id.clone(),
                runtime_epoch: caller_owner.runtime_epoch,
                now_millis: now + 5,
            };
            assert_eq!(port.begin_close(&close), Err(HostError::RecoveryRequired));
            port.begin_close(&close).unwrap();
            assert_eq!(
                port.wait_zero(&caller_owner),
                Err(HostError::RecoveryRequired)
            );
            let proof = port.wait_zero(&caller_owner).unwrap();
            let (error, proof) = port
                .finalize_close(&close, proof)
                .expect_err("the injected projection conflict requires recovery");
            assert_eq!(error, HostError::RecoveryRequired);
            port.finalize_close(&close, proof).unwrap();
            let result = (turn_id, first, retry);
            drop(port);
            drop(caller_owner);
            caller_finished
                .send(())
                .expect("recovery caller completion receiver retained");
            result
        })
        .expect("spawn recovery caller");

    let first = mailbox
        .wait_next_until(
            Some(mailbox_deadline_before(
                context.clock.as_ref(),
                whole_test_deadline,
            )),
            context.clock.as_ref(),
        )
        .unwrap()
        .expect("prepare command");
    let ControlMessage::Worker(first) = first else {
        panic!("worker command")
    };
    let mut state = FirstTurnWorkerState::Idle(completed)
        .handle(&context, first)
        .await;
    let FirstTurnWorkerState::Prepared { turn, .. } = &state else {
        panic!("prepared worker state")
    };
    assert_eq!(turn.active_turn.external_turn_id, None);
    assert_eq!(
        turn.active_turn.client_message_id.as_str(),
        turn.dispatch.turn_id().as_str()
    );
    assert_eq!(turn.parts.owner, owner);

    // The prepare command is handled above. Pump each of the following
    // fifteen commands so the final successful `FinalizeClose` can answer
    // its synchronous caller before this test joins that thread.
    for command_index in 0..15 {
        let command = mailbox
            .wait_next_until(
                Some(mailbox_deadline_before(
                    context.clock.as_ref(),
                    whole_test_deadline,
                )),
                context.clock.as_ref(),
            )
            .unwrap()
            .expect("worker command before recovery-test deadline");
        let ControlMessage::Worker(command) = command else {
            panic!("worker command")
        };
        if matches!(command_index, 0 | 4 | 6 | 9 | 11 | 13) {
            while publisher.take().is_some() {}
            publisher
                .publish_committed(crate::acp::intent::ProjectionChangedV1 {
                    project_id: project_id.clone(),
                    chat_id: SageChatId::parse("chat-foreign").unwrap(),
                    session_id: started.session_id.clone(),
                    projection_revision: ProjectionRevision::new(1).unwrap(),
                    child_projection_epoch: 0,
                })
                .unwrap();
        }
        state = state.handle(&context, command).await;
        if matches!(command_index, 0 | 4 | 6 | 9 | 11 | 13) {
            assert!(matches!(state, FirstTurnWorkerState::Recovery(_)));
            assert!(publisher.take().is_some());
        }
    }
    caller_completion
        .recv_timeout(remaining_before(whole_test_deadline))
        .expect("recovery caller exceeded the whole-test deadline");
    let (turn_id, first, retry) = caller.join().expect("recovery caller thread joined");
    assert!(matches!(state, FirstTurnWorkerState::Closed));
    assert_eq!(first, LiveWrite::ZeroBytes);
    assert_eq!(retry, LiveWrite::Complete);
    assert_eq!(cancel_count.load(Ordering::SeqCst), 1);
    // NoChildProcessControl does not launch a child. This is a protocol-boundary
    // check, not evidence about a real Windows process tree.
    assert_eq!(zero_checks.load(Ordering::SeqCst), 1);
    assert!(scheduler.active().is_none());
    let durable_state: (String, String, Option<i64>) = database
        .connection()
        .query_row(
            "SELECT state,delivery_state,terminal_at FROM turns WHERE id=?1",
            rusqlite::params![turn_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (durable_state.0.as_str(), durable_state.1.as_str()),
        ("deliveryUnknown", "completeBytes")
    );
    assert!(durable_state.2.is_some());
    let session_state: String = database
        .connection()
        .query_row(
            "SELECT state FROM agent_sessions WHERE id=?1",
            [started.session_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(session_state, "closed");
    let cancel_facts: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE turn_id=?1 AND kind='turnCancelIntent'",
            [turn_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(cancel_facts, 1);
    let send_facts: (i64, i64) = database
            .connection()
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM session_facts WHERE turn_id=?1 AND kind='sendStarted'),
                   (SELECT COUNT(*) FROM session_facts WHERE turn_id=?1 AND kind='sendCompleteBytes')",
                [turn_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
    assert_eq!(send_facts, (2, 1));
    let close_facts: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sessionCloseIntent'",
            [started.session_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(close_facts, 1);

    drop(state);
    drop(context);
    drop(_sessions);
    drop(scheduler);
    drop(journal);
    journal_task.finish_before(whole_test_deadline);
    assert_eq!(database_ownership.strong_count(), 1);
    drop(database);
    assert_eq!(database_ownership.strong_count(), 0);
    assert!(database_ownership.upgrade().is_none());
    drop(root);
}

#[test]
fn one_worker_reuses_the_same_session_for_two_store_minted_turns_then_closes() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let root = std::env::temp_dir().join(format!("sage-two-turns-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse("chat-two-turns").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES (?1,?2,1,'Two Turns',0,?3,?3)",
                rusqlite::params![chat_id.as_str(), project_id.as_str(), now],
            )
            .unwrap();
        let publisher = Arc::new(ProjectionMailbox::default());
        let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let scheduler = Scheduler::new();
        let sessions =
            SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
        let initial = crate::acp::store::load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let binding = "55".repeat(32);
        let artifact = "66".repeat(32);
        let started = sessions
            .start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: initial.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-two-start").unwrap(),
                binding_id: RequiredText::new("binding", "binding-two").unwrap(),
                binding_digest: binding.clone(),
                harness_version: None,
                model_id: None,
                now_millis: now,
            })
            .await
            .unwrap();
        let epoch = RuntimeEpoch::new(1).unwrap();
        let lease = sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-two-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: artifact.clone(),
                now_millis: now + 1,
            })
            .await
            .unwrap();
        let external_session = ExternalId::new("external", "external-two").unwrap();
        let mut activation_journal = CoordinatorActivationJournal::new(lease);
        let ready = activation_journal
            .commit_ready(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: binding,
                artifact_digest: artifact,
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities()).unwrap(),
                now_millis: now + 2,
            })
            .await
            .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
        };
        let prompt_ids = Arc::new(Mutex::new(Vec::new()));
        let cancel_count = Arc::new(AtomicUsize::new(0));
        let zero_checks = Arc::new(AtomicUsize::new(0));
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                SequencedAdapter {
                    binding_digest: [0x55; 32],
                    runtime_epoch: epoch,
                    session_id: started.session_id.clone(),
                    external_session_id: external_session.clone(),
                    pending_message: None,
                    prompt_ids: Arc::clone(&prompt_ids),
                    cancel_count: Arc::clone(&cancel_count),
                    terminal_states: Arc::new(Mutex::new(VecDeque::from([
                        TurnState::Completed,
                        TurnState::Cancelled,
                    ]))),
                    next_request: 0,
                    next_external_turn: 0,
                },
                activation_journal,
                Mapper,
                ActivatedSession {
                    binding_digest: [0x55; 32],
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    external_session_id: external_session,
                    runtime_epoch: epoch,
                    capabilities: capabilities(),
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
            transport: Transport(Mutex::new(VecDeque::from([
                FrameWriteOutcome::Complete,
                FrameWriteOutcome::Complete,
            ]))),
            control: NoChildProcessControl {
                zero_checks: Arc::clone(&zero_checks),
            },
            owner: owner.clone(),
            clock: Arc::new(SystemDriverClock),
        };
        let authority_state = Arc::new(Mutex::new(IntentSnapshot {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            projection_revision: ready.projection_revision,
            session_id: Some(started.session_id.clone()),
            runtime_epoch: Some(epoch),
            turn_id: None,
            turn_state: None,
            turn_revision: None,
        }));
        let authority = Arc::new(MutableAuthority(Arc::clone(&authority_state)));
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let context = FirstTurnWorkerContext {
            owner: owner.clone(),
            coordinator: SendCoordinator::new(authority, journal.clone(), publisher.clone()),
            journal: journal.clone(),
            publisher,
            clock: Arc::new(SystemDriverClock),
            mailbox: Arc::clone(&mailbox),
            session_recovery: None,
            zero_sequence: std::sync::atomic::AtomicU64::new(0),
        };
        let mut state = FirstTurnWorkerState::Idle(completed);

        macro_rules! step {
            ($command:expr) => {{
                let response = mailbox.submit_worker($command).unwrap();
                let message = mailbox
                    .wait_next_until(None, context.clock.as_ref())
                    .unwrap()
                    .expect("worker command");
                let ControlMessage::Worker(envelope) = message else {
                    panic!("worker envelope")
                };
                state = state.handle(&context, envelope).await;
                response.recv().unwrap()
            }};
        }

        let prepare = |intent: &'static str, expected: ProjectionRevision, at: i64| {
            WorkerCommand::PreparePrompt(HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id: project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: expected,
                    client_intent_id: ClientIntentId::parse(intent).unwrap(),
                    prompt: Prompt::new("prompt", format!("prompt-{intent}")).unwrap(),
                    now_millis: at,
                },
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
        };

        let WorkerReply::Prepared(turn_one) = step!(prepare(
            "intent-turn-one",
            ready.projection_revision,
            now + 3
        ))
        .unwrap()
        else {
            panic!("first prepared reply")
        };
        assert!(matches!(state, FirstTurnWorkerState::Prepared { .. }));
        let WorkerReply::Write(LiveWrite::Complete) = step!(WorkerCommand::DispatchPrompt {
            owner: owner.clone(),
            turn_id: turn_one.clone(),
        })
        .unwrap()
        else {
            panic!("first write")
        };
        step!(WorkerCommand::ObserveSend {
            owner: owner.clone(),
            turn_id: turn_one.clone(),
            observation: crate::acp::store::SendObservation::CompleteBytes,
        })
        .unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        let cursor_one = match state {
            FirstTurnWorkerState::Active(mut active) => {
                active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
                let cursor = active
                    .turn
                    .driver
                    .latest_durable_turn_cursor()
                    .cloned()
                    .expect("first durable terminal");
                state = FirstTurnWorkerState::Active(active);
                cursor
            }
            _ => panic!("first active state"),
        };
        assert_eq!(cursor_one.turn_id(), &turn_one);
        assert_eq!(cursor_one.state(), TurnState::Completed);
        assert_eq!(cursor_one.delivery_state(), DeliveryState::Correlated);
        authority_state.lock().unwrap().projection_revision = cursor_one.projection_revision();
        let terminal_first_cancel = HostCancelIntent {
            owner: owner.clone(),
            turn_id: turn_one.clone(),
            client_intent_id: ClientIntentId::parse("intent-terminal-first-cancel").unwrap(),
            now_millis: now + 4,
        };
        step!(WorkerCommand::BeginCancel(terminal_first_cancel.clone())).unwrap();
        step!(WorkerCommand::DispatchCancel(terminal_first_cancel)).unwrap();
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
        let terminal_response = mailbox
            .submit_worker(WorkerCommand::AcceptTerminal {
                owner: owner.clone(),
                cursor: cursor_one.clone(),
            })
            .unwrap();
        let queued_response = mailbox.submit_worker(WorkerCommand::Shutdown).unwrap();
        for expected in [&terminal_response, &queued_response] {
            let message = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("queued terminal recovery command");
            let ControlMessage::Worker(envelope) = message else {
                panic!("worker envelope")
            };
            state = state.handle(&context, envelope).await;
            assert!(matches!(
                expected.recv().unwrap(),
                Err(HostError::RecoveryRequired)
            ));
            assert!(matches!(state, FirstTurnWorkerState::Recovery(_)));
        }
        step!(WorkerCommand::AcceptTerminal {
            owner: owner.clone(),
            cursor: cursor_one.clone(),
        })
        .unwrap();
        assert!(matches!(state, FirstTurnWorkerState::Reusable(_)));

        let WorkerReply::Prepared(turn_two) = step!(prepare(
            "intent-turn-two",
            cursor_one.projection_revision(),
            now + 4
        ))
        .unwrap()
        else {
            panic!("second prepared reply")
        };
        assert_ne!(turn_one, turn_two);
        let WorkerReply::Write(LiveWrite::Complete) = step!(WorkerCommand::DispatchPrompt {
            owner: owner.clone(),
            turn_id: turn_two.clone(),
        })
        .unwrap()
        else {
            panic!("second write")
        };
        step!(WorkerCommand::ObserveSend {
            owner: owner.clone(),
            turn_id: turn_two.clone(),
            observation: crate::acp::store::SendObservation::CompleteBytes,
        })
        .unwrap();
        let terminal_between_cancel = HostCancelIntent {
            owner: owner.clone(),
            turn_id: turn_two.clone(),
            client_intent_id: ClientIntentId::parse("intent-terminal-between-cancel").unwrap(),
            now_millis: now + 5,
        };
        step!(WorkerCommand::BeginCancel(terminal_between_cancel.clone())).unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        let cursor_two = match state {
            FirstTurnWorkerState::Active(mut active) => {
                active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
                let cursor = active
                    .turn
                    .driver
                    .latest_durable_turn_cursor()
                    .cloned()
                    .expect("second durable terminal");
                state = FirstTurnWorkerState::Active(active);
                cursor
            }
            _ => panic!("second active state"),
        };
        assert_eq!(cursor_two.turn_id(), &turn_two);
        assert_eq!(cursor_two.state(), TurnState::Cancelled);
        authority_state.lock().unwrap().projection_revision = cursor_two.projection_revision();
        step!(WorkerCommand::DispatchCancel(terminal_between_cancel)).unwrap();
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
        step!(WorkerCommand::AcceptTerminal {
            owner: owner.clone(),
            cursor: cursor_two.clone(),
        })
        .unwrap();
        assert!(matches!(state, FirstTurnWorkerState::Reusable(_)));

        let close = crate::acp::store::CloseSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: cursor_two.projection_revision(),
            client_intent_id: ClientIntentId::parse("intent-two-close").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            now_millis: now + 6,
        };
        step!(WorkerCommand::BeginClose(close.clone())).unwrap();
        let WorkerReply::Zero(proof) = step!(WorkerCommand::WaitZero {
            owner: owner.clone(),
        })
        .unwrap()
        else {
            panic!("zero proof")
        };
        step!(WorkerCommand::FinalizeClose {
            command: close,
            proof,
        })
        .unwrap();
        assert!(matches!(state, FirstTurnWorkerState::Closed));

        assert_eq!(
            *prompt_ids.lock().unwrap(),
            [turn_one.as_str().to_owned(), turn_two.as_str().to_owned()]
        );
        assert_eq!(zero_checks.load(Ordering::SeqCst), 1);
        assert!(scheduler.active().is_none());
        {
            let connection = database.connection();
            let mut statement = connection
                .prepare(
                    "SELECT id,state,delivery_state,terminal_at FROM turns
                     WHERE session_id=?1 ORDER BY ordinal LIMIT 3",
                )
                .unwrap();
            let rows = statement
                .query_map([started.session_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].0, turn_one.as_str());
            assert_eq!(rows[1].0, turn_two.as_str());
            for (index, row) in rows.into_iter().enumerate() {
                assert_eq!(row.1, if index == 0 { "completed" } else { "cancelled" });
                assert_eq!(
                    row.2,
                    if index == 0 {
                        "correlated"
                    } else {
                        "completeBytes"
                    }
                );
                assert!(row.3.is_some());
            }
            let facts: (i64, i64) = connection
                .query_row(
                    "SELECT
                       (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sessionCloseIntent'),
                       (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='turnCancelIntent')",
                    [started.session_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(facts, (1, 1));
        }

        journal.close_when_drained();
        task.await.unwrap();
        drop(database);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn activated_idle_session_closes_without_fabricating_a_turn() {
    close_before_first_write(CloseBeforeWriteCase::Idle);
}

#[test]
fn idle_reader_eof_blocks_a_queued_prompt_without_fabricating_a_turn() {
    close_before_first_write(CloseBeforeWriteCase::IdleReaderEofPrompt);
}

#[test]
fn idle_reader_eof_allows_a_queued_close_to_complete() {
    close_before_first_write(CloseBeforeWriteCase::IdleReaderEofClose);
}

#[test]
fn idle_reader_eof_blocks_configuration_before_it_writes() {
    close_before_first_write(CloseBeforeWriteCase::IdleReaderEofConfigure);
}

#[test]
fn idle_reader_eof_preserves_a_queued_process_exit_at_its_deadline() {
    close_before_first_write(CloseBeforeWriteCase::IdleReaderEofThenProcessExitDeadline);
}

#[test]
fn idle_reader_eof_deadline_exits_without_spinning() {
    close_before_first_write(CloseBeforeWriteCase::IdleReaderEofDeadline);
}

#[test]
fn idle_pending_config_reader_eof_fails_at_its_deadline() {
    close_before_first_write(CloseBeforeWriteCase::IdlePendingConfigReaderEofDeadline);
}

#[test]
fn prepared_reader_eof_blocks_a_queued_prompt_dispatch_without_writing() {
    close_before_first_write(CloseBeforeWriteCase::PreparedReaderEofDispatch);
}

#[test]
fn active_non_stdout_process_exit_resolves_at_its_deadline() {
    close_before_first_write(CloseBeforeWriteCase::ActiveProcessExitDeadline);
}

#[test]
fn active_ready_for_write_process_exit_resolves_at_its_deadline() {
    close_before_first_write(CloseBeforeWriteCase::ActiveReadyForWriteProcessExitDeadline);
}

#[test]
fn active_reader_eof_preserves_a_queued_process_exit_at_its_deadline() {
    close_before_first_write(CloseBeforeWriteCase::ActiveReaderEofThenProcessExitDeadline);
}

#[test]
fn reusable_reader_eof_preserves_a_queued_process_exit_at_its_deadline() {
    close_before_first_write(CloseBeforeWriteCase::ReusableReaderEofThenProcessExitDeadline);
}

#[test]
fn reusable_reader_eof_deadline_exits_without_spinning() {
    close_before_first_write(CloseBeforeWriteCase::ReusableReaderEofDeadline);
}

#[test]
fn reusable_pending_config_reader_eof_fails_at_its_deadline() {
    close_before_first_write(CloseBeforeWriteCase::ReusablePendingConfigReaderEofDeadline);
}

#[test]
fn prepared_turn_closes_as_cancelled_without_writing_prompt_or_cancel() {
    close_before_first_write(CloseBeforeWriteCase::Prepared);
}

#[test]
fn prepared_runner_accepts_early_mcp_status_before_dispatch() {
    close_before_first_write(CloseBeforeWriteCase::PreparedEarlyStdout);
}

#[test]
fn prepared_runner_buffers_early_browser_failure_before_dispatch() {
    close_before_first_write(CloseBeforeWriteCase::PreparedEarlyFailedStdout);
}

#[test]
fn prepared_runner_rejects_unrelated_non_ignorable_stdout() {
    close_before_first_write(CloseBeforeWriteCase::PreparedUnexpectedStdout);
}

#[test]
fn applied_begin_send_commit_unknown_reconciles_without_prompt_replay() {
    close_before_first_write(CloseBeforeWriteCase::BeginSendAppliedUnknown);
}

#[test]
fn absent_begin_send_commit_unknown_reconciles_without_prompt_replay() {
    close_before_first_write(CloseBeforeWriteCase::BeginSendNotAppliedUnknown);
}

#[test]
fn final_stdout_commit_unknown_after_process_exit_reconciles_before_terminalizing() {
    close_before_first_write(CloseBeforeWriteCase::FinalStdoutAppliedUnknown);
}

#[test]
fn absent_final_stdout_commit_unknown_after_process_exit_reconciles_before_terminalizing() {
    close_before_first_write(CloseBeforeWriteCase::FinalStdoutNotAppliedUnknown);
}

fn close_before_first_write(case: CloseBeforeWriteCase) {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(move || close_before_first_write_on_stack(case))
        .unwrap()
        .join()
        .unwrap();
}

fn close_before_first_write_on_stack(case: CloseBeforeWriteCase) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let root = TempRoot::new("sage-idle-close");
        fs::create_dir_all(root.path()).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(root.path()).unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse("chat-idle-close").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES (?1,?2,1,'Idle Close',0,?3,?3)",
                rusqlite::params![chat_id.as_str(), project_id.as_str(), now],
            )
            .unwrap();
        let publisher = Arc::new(ProjectionMailbox::default());
        let begin_send_injection = Arc::new(Mutex::new(None));
        let adapter_inbound_batch_injection = Arc::new(Mutex::new(None));
        let active_turn_lookups = Arc::new(AtomicUsize::new(0));
        let (journal, runner) = JournalHandle::new(CommitUnknownOnceStore {
            inner: DatabaseAcpStore::new(database.clone()),
            begin_send: Arc::clone(&begin_send_injection),
            adapter_inbound_batch: Arc::clone(&adapter_inbound_batch_injection),
            active_turn_lookups: Arc::clone(&active_turn_lookups),
        });
        let journal_thread = BoundedThread::spawn("idle-close-journal", move || {
            runner.run_blocking()
        })
        .expect("spawn idle-close journal");
        let journal_task = JournalThreadCleanup {
            journal: journal.clone(),
            thread: Some(journal_thread),
        };
        let scheduler = Scheduler::new();
        let sessions =
            SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
        let initial = crate::acp::store::load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let binding = "33".repeat(32);
        let artifact = "44".repeat(32);
        let started = sessions
            .start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: initial.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-idle-start").unwrap(),
                binding_id: RequiredText::new("binding", "binding-idle").unwrap(),
                binding_digest: binding.clone(),
                harness_version: None,
                model_id: None,
                now_millis: now,
            })
            .await
            .unwrap();
        let epoch = RuntimeEpoch::new(1).unwrap();
        let lease = sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-idle-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: artifact.clone(),
                now_millis: now + 1,
            })
            .await
            .unwrap();
        let external_session = ExternalId::new("external", "external-idle").unwrap();
        let mut activation_journal = CoordinatorActivationJournal::new(lease);
        let ready = activation_journal
            .commit_ready(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: binding,
                artifact_digest: artifact,
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities()).unwrap(),
                now_millis: now + 2,
            })
            .await
            .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
        };
        let zero_checks = Arc::new(AtomicUsize::new(0));
        let cancel_count = Arc::new(AtomicUsize::new(0));
        let deadline_clock = Arc::new(DeadlineClock::default());
        deadline_clock.set(now);
        let clock: Arc<dyn DriverClock> = if matches!(
            case,
            CloseBeforeWriteCase::ActiveProcessExitDeadline
                | CloseBeforeWriteCase::ActiveReadyForWriteProcessExitDeadline
                | CloseBeforeWriteCase::ActiveReaderEofThenProcessExitDeadline
                | CloseBeforeWriteCase::IdleReaderEofThenProcessExitDeadline
                | CloseBeforeWriteCase::IdleReaderEofDeadline
                | CloseBeforeWriteCase::IdlePendingConfigReaderEofDeadline
                | CloseBeforeWriteCase::ReusableReaderEofThenProcessExitDeadline
                | CloseBeforeWriteCase::ReusableReaderEofDeadline
                | CloseBeforeWriteCase::ReusablePendingConfigReaderEofDeadline
        ) {
            Arc::clone(&deadline_clock) as Arc<dyn DriverClock>
        } else {
            Arc::new(SystemDriverClock)
        };
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                Adapter {
                    cancel_count: Arc::clone(&cancel_count),
                    final_stdout: (case.final_stdout_injection().is_some()
                        || matches!(
                            case,
                            CloseBeforeWriteCase::ReusableReaderEofThenProcessExitDeadline
                                | CloseBeforeWriteCase::EarlyStdoutBeforeObservation
                                | CloseBeforeWriteCase::ReusableReaderEofDeadline
                                | CloseBeforeWriteCase::ReusablePendingConfigReaderEofDeadline
                        ))
                        .then(|| AdapterIdentity {
                            binding_digest: [0x33; 32],
                            runtime_epoch: epoch,
                            session_id: started.session_id.clone(),
                            external_id: external_session.clone(),
                            turn_id: Some(ExternalId::new("externalTurn", "final-stdout").unwrap()),
                            turn_client_message_id: None,
                        }),
                    config_pending: false,
                },
                activation_journal,
                Mapper,
                ActivatedSession {
                    // A prepared close must not bind an adapter pump. The zero
                    // digest would make that admission fail deterministically.
                    binding_digest: if case == CloseBeforeWriteCase::Prepared {
                        [0; 32]
                    } else {
                        [0x33; 32]
                    },
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    external_session_id: external_session,
                    runtime_epoch: epoch,
                    capabilities: capabilities(),
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
            transport: Transport(Mutex::new(VecDeque::new())),
            control: NoChildProcessControl {
                zero_checks: Arc::clone(&zero_checks),
            },
            owner: owner.clone(),
            clock: Arc::clone(&clock),
        };
        let authority = Arc::new(Authority(IntentSnapshot {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            projection_revision: ready.projection_revision,
            session_id: Some(started.session_id.clone()),
            runtime_epoch: Some(epoch),
            turn_id: None,
            turn_state: None,
            turn_revision: None,
        }));
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let mut context = FirstTurnWorkerContext {
            owner: owner.clone(),
            coordinator: SendCoordinator::new(authority, journal.clone(), publisher.clone()),
            journal: journal.clone(),
            publisher,
            clock,
            mailbox: Arc::clone(&mailbox),
            session_recovery: None,
            zero_sequence: std::sync::atomic::AtomicU64::new(0),
        };
        let mut state = FirstTurnWorkerState::Idle(completed);
        if case == CloseBeforeWriteCase::IdlePendingConfigReaderEofDeadline {
            let deadline = now
                + i64::try_from(crate::acp::limits::LIMITS.deadlines_ms.tree_termination)
                    .unwrap();
            let reason = configured_runner_reaches_reader_eof_deadline(
                state,
                context,
                Arc::clone(&mailbox),
                owner.clone(),
                Arc::clone(&deadline_clock),
                deadline,
            );
            assert_eq!(reason, TreeExit::Exited { code: None });
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if matches!(
            case,
            CloseBeforeWriteCase::IdleReaderEofThenProcessExitDeadline
                | CloseBeforeWriteCase::IdleReaderEofDeadline
        ) {
            let deadline = now
                + i64::try_from(crate::acp::limits::LIMITS.deadlines_ms.tree_termination)
                    .unwrap();
            mailbox
                .signal_reader_terminal(ReaderTerminal::Eof)
                .unwrap();
            let reads_before_hook = deadline_clock.read_count();
            if case == CloseBeforeWriteCase::IdleReaderEofThenProcessExitDeadline {
                deadline_clock
                    .advance_after_next_read_with_process_exit(Arc::clone(&mailbox), deadline);
            } else {
                deadline_clock.advance_after_next_read(deadline);
            }
            let (_, reason) = tokio::time::timeout(
                Duration::from_millis(50),
                super::live_runner::drive_for_test(state, &context),
            )
            .await
            .expect("idle runner reaches the reader EOF deadline");
            assert_eq!(deadline_clock.hook_read(), reads_before_hook + 1);
            assert_eq!(
                reason,
                if case == CloseBeforeWriteCase::IdleReaderEofThenProcessExitDeadline {
                    TreeExit::Exited { code: Some(17) }
                } else {
                    TreeExit::Exited { code: None }
                }
            );
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if case == CloseBeforeWriteCase::IdleReaderEofPrompt {
            let response = mailbox
                .submit_worker(WorkerCommand::PreparePrompt(HostPromptIntent {
                    command: PrepareTurnCommand {
                        project_id: project_id.clone(),
                        project_generation: ProjectGeneration::ZERO,
                        chat_id: chat_id.clone(),
                        session_id: started.session_id.clone(),
                        runtime_epoch: epoch,
                        expected_projection_revision: ready.projection_revision,
                        client_intent_id: ClientIntentId::parse("intent-eof-prompt").unwrap(),
                        prompt: Prompt::new("prompt", "must-not-write-after-eof").unwrap(),
                        now_millis: now + 3,
                    },
                    access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
                }))
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("queued prompt");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            mailbox
                .signal_reader_terminal(ReaderTerminal::Eof)
                .unwrap();
            assert!(matches!(
                mailbox.wait_next_until(None, context.clock.as_ref()).unwrap(),
                Some(ControlMessage::ReaderTerminal(ReaderTerminal::Eof))
            ));
            state = state.handle(&context, command).await;
            assert!(matches!(
                response.recv_timeout(tree_termination_deadline()),
                Ok(Err(HostError::RecoveryRequired))
            ));
            assert!(matches!(state, FirstTurnWorkerState::Idle(_)));
            let turns: i64 = database
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM turns WHERE session_id=?1",
                    [started.session_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(turns, 0);
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if case == CloseBeforeWriteCase::IdleReaderEofClose {
            let close = crate::acp::store::CloseSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: ready.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-eof-close").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                now_millis: now + 3,
            };
            mailbox
                .signal_reader_terminal(ReaderTerminal::Eof)
                .unwrap();
            assert!(matches!(
                mailbox.wait_next_until(None, context.clock.as_ref()).unwrap(),
                Some(ControlMessage::ReaderTerminal(ReaderTerminal::Eof))
            ));
            let begin = mailbox
                .submit_worker(WorkerCommand::BeginClose(close.clone()))
                .unwrap();
            let begin_command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("queued begin close");
            let ControlMessage::Worker(begin_command) = begin_command else {
                panic!("worker command")
            };
            state = state.handle(&context, begin_command).await;
            assert!(matches!(
                begin.recv_timeout(tree_termination_deadline()),
                Ok(Ok(WorkerReply::Unit))
            ));
            let wait_zero = mailbox
                .submit_worker(WorkerCommand::WaitZero {
                    owner: owner.clone(),
                })
                .unwrap();
            let wait_zero_command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("queued wait zero");
            let ControlMessage::Worker(wait_zero_command) = wait_zero_command else {
                panic!("worker command")
            };
            state = state.handle(&context, wait_zero_command).await;
            let proof = match wait_zero.recv_timeout(tree_termination_deadline()) {
                Ok(Ok(WorkerReply::Zero(proof))) => proof,
                _ => panic!("wait zero reply"),
            };
            let finalize = mailbox
                .submit_worker(WorkerCommand::FinalizeClose {
                    command: close,
                    proof,
                })
                .unwrap();
            let finalize_command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("queued finalize close");
            let ControlMessage::Worker(finalize_command) = finalize_command else {
                panic!("worker command")
            };
            state = state.handle(&context, finalize_command).await;
            assert!(matches!(
                finalize.recv_timeout(tree_termination_deadline()),
                Ok(Ok(WorkerReply::Unit))
            ));
            assert!(matches!(state, FirstTurnWorkerState::Closed));
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if case == CloseBeforeWriteCase::IdleReaderEofConfigure {
            let completion_observation = Arc::new(Mutex::new(None));
            let observed_completion = Arc::clone(&completion_observation);
            let response = mailbox
                .submit_worker(WorkerCommand::ConfigureSession {
                    intent: crate::acp::session_config::ConfigIntent {
                        owner: owner.clone(),
                        expected_config_revision:
                            crate::acp::session_config::ConfigRevision::first(),
                        mutation: crate::acp::session_config::ConfigMutation::Mode(
                            crate::acp::session_config::ConfigId::new("mode", "code").unwrap(),
                        ),
                    },
                    completion: crate::acp::runtime::ConfigCompletionSink::new(move |result| {
                        *observed_completion.lock().unwrap() = Some(result);
                    }),
                })
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("queued configuration");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            mailbox
                .signal_reader_terminal(ReaderTerminal::Eof)
                .unwrap();
            assert!(matches!(
                mailbox.wait_next_until(None, context.clock.as_ref()).unwrap(),
                Some(ControlMessage::ReaderTerminal(ReaderTerminal::Eof))
            ));
            let (command, responder) = command.into_parts();
            let WorkerCommand::ConfigureSession { intent, completion } = command else {
                panic!("configuration command")
            };
            let result = state.begin_configure(
                &context,
                &intent,
                Instant::now() + tree_termination_deadline(),
            );
            responder.respond(result.clone().map(|()| WorkerReply::Unit));
            completion.complete(result);
            assert!(matches!(
                response.recv_timeout(tree_termination_deadline()),
                Ok(Err(HostError::RecoveryRequired))
            ));
            assert_eq!(
                completion_observation.lock().unwrap().take(),
                Some(Err(HostError::RecoveryRequired))
            );
            assert!(!state.config_pending());
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        let expected_projection_revision = if case.prepares_turn() {
            let response = mailbox
                .submit_worker(WorkerCommand::PreparePrompt(HostPromptIntent {
                    command: PrepareTurnCommand {
                        project_id: project_id.clone(),
                        project_generation: ProjectGeneration::ZERO,
                        chat_id: chat_id.clone(),
                        session_id: started.session_id.clone(),
                        runtime_epoch: epoch,
                        expected_projection_revision: ready.projection_revision,
                        client_intent_id: ClientIntentId::parse("intent-prepared-close-turn")
                            .unwrap(),
                        prompt: Prompt::new("prompt", "must-not-write").unwrap(),
                        now_millis: now + 3,
                    },
                    access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
                }))
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("prepared close command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(
                response
                    .recv_timeout(tree_termination_deadline())
                    .expect("prepared close reply before deadline"),
                Ok(WorkerReply::Prepared(_))
            ));
            assert!(matches!(state, FirstTurnWorkerState::Prepared { .. }));
            crate::acp::store::load_chat_page(
                &database.connection(),
                project_id.clone(),
                chat_id.clone(),
                None,
            )
            .unwrap()
            .projection_revision
        } else {
            ready.projection_revision
        };
        if matches!(
            case,
            CloseBeforeWriteCase::PreparedEarlyStdout
                | CloseBeforeWriteCase::PreparedEarlyFailedStdout
                | CloseBeforeWriteCase::PreparedUnexpectedStdout
        ) {
            let frame: &[u8] = match case {
                CloseBeforeWriteCase::PreparedEarlyStdout => br#"{"method":"mcpServer/startupStatus/updated","params":{"name":"sage-browser","threadId":"external-idle","status":"starting","error":null,"failureReason":null}}"#,
                CloseBeforeWriteCase::PreparedEarlyFailedStdout => br#"{"method":"mcpServer/startupStatus/updated","params":{"name":"sage-browser","threadId":"external-idle","status":"failed","error":"browser helper unavailable","failureReason":null}}"#,
                CloseBeforeWriteCase::PreparedUnexpectedStdout => br#"{"method":"turn/started","params":{"threadId":"external-idle","turn":{"id":"unexpected","status":"inProgress"}}}"#,
                _ => unreachable!(),
            };
            let mut chunk = frame.to_vec();
            chunk.push(b'\n');
            mailbox.try_push_chunk(chunk).unwrap();
            let shutdown = if case == CloseBeforeWriteCase::PreparedUnexpectedStdout {
                None
            } else {
                let shutdown_mailbox = Arc::clone(&mailbox);
                let dispatch_owner = owner.clone();
                let turn_id = match &state {
                    FirstTurnWorkerState::Prepared { turn, .. } => turn.dispatch.turn_id().clone(),
                    _ => panic!("prepared state"),
                };
                Some(std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(10));
                    let dispatch = shutdown_mailbox
                        .submit_worker(WorkerCommand::DispatchPrompt {
                            owner: dispatch_owner,
                            turn_id,
                        })
                        .unwrap();
                    assert!(matches!(
                        dispatch.recv_timeout(tree_termination_deadline()),
                        Ok(Ok(WorkerReply::Write(LiveWrite::Complete)))
                    ));
                    shutdown_mailbox.submit_worker(WorkerCommand::Shutdown).unwrap()
                }))
            };
            let (next, reason) = tokio::time::timeout(
                Duration::from_millis(200),
                super::live_runner::drive_for_test(state, &context),
            )
            .await
            .expect("prepared runner handles early status before shutdown");
            if let Some(shutdown) = shutdown {
                drop(shutdown.join().unwrap());
                assert_eq!(reason, TreeExit::ParentShutdown);
                assert!(matches!(next, FirstTurnWorkerState::Active(_)));
            } else {
                assert_eq!(reason, TreeExit::ProtocolFailure);
                assert!(matches!(next, FirstTurnWorkerState::Prepared { .. }));
            }
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if case == CloseBeforeWriteCase::PreparedReaderEofDispatch {
            let response = mailbox
                .submit_worker(WorkerCommand::DispatchPrompt {
                    owner: owner.clone(),
                    turn_id: match &state {
                        FirstTurnWorkerState::Prepared { turn, .. } => {
                            turn.dispatch.turn_id().clone()
                        }
                        _ => panic!("prepared state"),
                    },
                })
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("queued prompt dispatch");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            mailbox
                .signal_reader_terminal(ReaderTerminal::Eof)
                .unwrap();
            assert!(matches!(
                mailbox.wait_next_until(None, context.clock.as_ref()).unwrap(),
                Some(ControlMessage::ReaderTerminal(ReaderTerminal::Eof))
            ));
            state = state.handle(&context, command).await;
            assert!(matches!(
                response.recv_timeout(tree_termination_deadline()),
                Ok(Err(HostError::RecoveryRequired))
            ));
            assert!(matches!(state, FirstTurnWorkerState::Prepared { .. }));
            let turn: (String, String) = database
                .connection()
                .query_row(
                    "SELECT state,delivery_state FROM turns WHERE session_id=?1",
                    [started.session_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(turn, ("queued".to_owned(), "notWritten".to_owned()));
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if case.dispatches_prompt() {
            if let Some(applied) = case.begin_send_injection() {
                *begin_send_injection.lock().unwrap() = Some(applied);
            }
            let response = mailbox
                .submit_worker(WorkerCommand::DispatchPrompt {
                    owner: owner.clone(),
                    turn_id: match &state {
                        FirstTurnWorkerState::Prepared { turn, .. } => {
                            turn.dispatch.turn_id().clone()
                        }
                        _ => panic!("prepared state"),
                    },
                })
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("commit-unknown dispatch command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            if case.begin_send_injection().is_some() {
                assert!(matches!(
                    response
                        .recv_timeout(tree_termination_deadline())
                        .expect("unknown dispatch reply before deadline"),
                    Err(HostError::RecoveryRequired)
                ));
                assert!(matches!(
                    &state,
                    FirstTurnWorkerState::Active(active) if active.durable_mutation_unknown()
                ));
            } else {
                assert!(matches!(
                    response
                        .recv_timeout(tree_termination_deadline())
                        .expect("prompt write reply before deadline"),
                    Ok(WorkerReply::Write(_))
                ));
                assert!(matches!(&state, FirstTurnWorkerState::Active(_)));
            }
        }
        if case == CloseBeforeWriteCase::EarlyStdoutBeforeObservation {
            write_observation_tests::early_stdout_before_observation(state, &mut context, &database).await;
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if case == CloseBeforeWriteCase::ActiveReadyForWriteProcessExitDeadline {
            let turn_id = match &state {
                FirstTurnWorkerState::Active(active) => active.turn.dispatch.turn_id().clone(),
                _ => panic!("active state"),
            };
            let response = mailbox
                .submit_worker(WorkerCommand::ObserveSend {
                    owner: owner.clone(),
                    turn_id,
                    observation: crate::acp::store::SendObservation::ZeroBytes,
                })
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("zero-byte observation command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(
                response.recv_timeout(tree_termination_deadline()),
                Ok(Ok(WorkerReply::Unit))
            ));
            assert!(matches!(
                &state,
                FirstTurnWorkerState::Active(active)
                    if active.ready_for_write() && !active.stdout_allowed()
            ));
        }
        if case == CloseBeforeWriteCase::ActiveProcessExitDeadline {
            assert!(matches!(
                &state,
                FirstTurnWorkerState::Active(active)
                    if !active.ready_for_write() && !active.stdout_allowed()
            ));
        }
        if matches!(
            case,
            CloseBeforeWriteCase::ReusableReaderEofThenProcessExitDeadline
                | CloseBeforeWriteCase::ReusableReaderEofDeadline
                | CloseBeforeWriteCase::ReusablePendingConfigReaderEofDeadline
        ) {
            let turn_id = match &state {
                FirstTurnWorkerState::Active(active) => active.turn.dispatch.turn_id().clone(),
                _ => panic!("active state"),
            };
            let response = mailbox
                .submit_worker(WorkerCommand::ObserveSend {
                    owner: owner.clone(),
                    turn_id,
                    observation: crate::acp::store::SendObservation::CompleteBytes,
                })
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("complete-byte observation command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(
                response.recv_timeout(tree_termination_deadline()),
                Ok(Ok(WorkerReply::Unit))
            ));
            mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
            let cursor = match state {
                FirstTurnWorkerState::Active(mut active) => {
                    active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
                    let cursor = active
                        .turn
                        .driver
                        .latest_durable_turn_cursor()
                        .cloned()
                        .expect("durable terminal cursor");
                    state = FirstTurnWorkerState::Active(active);
                    cursor
                }
                _ => panic!("active state"),
            };
            let response = mailbox
                .submit_worker(WorkerCommand::AcceptTerminal {
                    owner: owner.clone(),
                    cursor,
                })
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("accept terminal command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(
                response.recv_timeout(tree_termination_deadline()),
                Ok(Ok(WorkerReply::Unit))
            ));
            assert!(matches!(state, FirstTurnWorkerState::Reusable(_)));
            let deadline = now
                + i64::try_from(crate::acp::limits::LIMITS.deadlines_ms.tree_termination)
                    .unwrap();
            if case == CloseBeforeWriteCase::ReusablePendingConfigReaderEofDeadline {
                let reason = configured_runner_reaches_reader_eof_deadline(
                    state,
                    context,
                    Arc::clone(&mailbox),
                    owner.clone(),
                    Arc::clone(&deadline_clock),
                    deadline,
                );
                assert_eq!(reason, TreeExit::Exited { code: None });
                drop(sessions);
                drop(scheduler);
                drop(journal);
                journal_task.finish();
                drop(database);
                return;
            }
            mailbox
                .signal_reader_terminal(ReaderTerminal::Eof)
                .unwrap();
            let reads_before_hook = deadline_clock.read_count();
            if case == CloseBeforeWriteCase::ReusableReaderEofThenProcessExitDeadline {
                deadline_clock
                    .advance_after_next_read_with_process_exit(Arc::clone(&mailbox), deadline);
            } else {
                deadline_clock.advance_after_next_read(deadline);
            }
            let (state, reason) = tokio::time::timeout(
                Duration::from_millis(50),
                super::live_runner::drive_for_test(state, &context),
            )
            .await
            .expect("reusable runner reaches the reader EOF deadline");
            assert_eq!(deadline_clock.hook_read(), reads_before_hook + 1);
            assert_eq!(
                reason,
                if case == CloseBeforeWriteCase::ReusableReaderEofThenProcessExitDeadline {
                    TreeExit::Exited { code: Some(17) }
                } else {
                    TreeExit::Exited { code: None }
                }
            );
            assert!(matches!(state, FirstTurnWorkerState::Reusable(_)));
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if case == CloseBeforeWriteCase::ActiveReaderEofThenProcessExitDeadline {
            assert!(matches!(
                &state,
                FirstTurnWorkerState::Active(active)
                    if !active.ready_for_write() && !active.stdout_allowed()
            ));
            let deadline = now
                + i64::try_from(crate::acp::limits::LIMITS.deadlines_ms.tree_termination)
                    .unwrap();
            mailbox
                .signal_reader_terminal(ReaderTerminal::Eof)
                .unwrap();
            let reads_before_hook = deadline_clock.read_count();
            deadline_clock.advance_after_next_read_with_process_exit(Arc::clone(&mailbox), deadline);
            let (state, reason) = tokio::time::timeout(
                Duration::from_millis(50),
                super::live_runner::drive_for_test(state, &context),
            )
            .await
            .expect("runner exits after the queued process exit is reconciled");
            assert_eq!(deadline_clock.hook_read(), reads_before_hook + 1);
            assert_eq!(reason, TreeExit::Exited { code: Some(17) });
            let FirstTurnWorkerState::Active(active) = state else {
                panic!("active state")
            };
            assert_eq!(
                active.turn.driver.terminal_outcome().unwrap().reason,
                crate::acp::lifecycle::TurnTerminalReason::HarnessExited
            );
            let persisted: (String, Option<String>) = database
                .connection()
                .query_row(
                    "SELECT kind,message FROM acp_turn_terminal_reasons WHERE session_id=?1",
                    [started.session_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(
                persisted,
                (
                    "harnessExited".to_owned(),
                    Some("harness process exited with code 17".to_owned()),
                )
            );
            drop(active);
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if matches!(
            case,
            CloseBeforeWriteCase::ActiveProcessExitDeadline
                | CloseBeforeWriteCase::ActiveReadyForWriteProcessExitDeadline
        ) {
            mailbox
                .signal_process_exit(TreeExit::Exited { code: Some(17) })
                .unwrap();
            let FirstTurnWorkerState::Active(active) = &mut state else {
                panic!("active state")
            };
            active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
            let deadline = active
                .turn
                .driver
                .pending_exit_deadline_millis()
                .expect("pending exit deadline");
            deadline_clock.set(deadline);
            let (_, reason) = tokio::time::timeout(
                Duration::from_millis(50),
                super::live_runner::drive_for_test(state, &context),
            )
            .await
            .expect("runner exits at the process-exit deadline");
            assert_eq!(reason, TreeExit::Exited { code: Some(17) });
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        if let Some(applied) = case.final_stdout_injection() {
            let turn_id = match &state {
                FirstTurnWorkerState::Active(active) => active.turn.dispatch.turn_id().clone(),
                _ => panic!("active state"),
            };
            let response = mailbox
                .submit_worker(WorkerCommand::ObserveSend {
                    owner: owner.clone(),
                    turn_id,
                    observation: crate::acp::store::SendObservation::CompleteBytes,
                })
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("observe send command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(
                response.recv_timeout(tree_termination_deadline()),
                Ok(Ok(WorkerReply::Unit))
            ));
            assert!(matches!(
                &state,
                FirstTurnWorkerState::Active(active) if active.stdout_allowed()
            ));
            *adapter_inbound_batch_injection.lock().unwrap() = Some(applied);
            mailbox
                .signal_process_exit(TreeExit::Exited { code: Some(17) })
                .unwrap();
            let FirstTurnWorkerState::Active(active) = &mut state else {
                panic!("active state")
            };
            active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
            assert!(active.turn.driver.teardown_cause().is_none());
            mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
            let (state, reason) = super::live_runner::drive_for_test(state, &context).await;
            assert_eq!(reason, TreeExit::Exited { code: Some(17) });
            assert!(adapter_inbound_batch_injection.lock().unwrap().is_none());
            let FirstTurnWorkerState::Active(active) = state else {
                panic!("active state")
            };
            assert_eq!(
                active.turn.driver.terminal_outcome().unwrap().reason,
                crate::acp::lifecycle::TurnTerminalReason::HarnessExited
            );
            assert!(!active.durable_mutation_unknown());
            assert_eq!(active_turn_lookups.load(Ordering::SeqCst), 1);
            let turn: (String, String) = database
                .connection()
                .query_row(
                    "SELECT state,delivery_state FROM turns WHERE session_id=?1",
                    [started.session_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let expected = if applied {
                ("completed".to_owned(), "correlated".to_owned())
            } else {
                ("deliveryUnknown".to_owned(), "completeBytes".to_owned())
            };
            assert_eq!(turn, expected);
            if !applied {
                let persisted: (String, Option<String>) = database
                    .connection()
                    .query_row(
                        "SELECT kind,message FROM acp_turn_terminal_reasons WHERE session_id=?1",
                        [started.session_id.as_str()],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(
                    persisted,
                    (
                        "harnessExited".to_owned(),
                        Some("harness process exited with code 17".to_owned()),
                    )
                );
                let page = crate::acp::store::load_chat_page(
                    &database.connection(),
                    project_id.clone(),
                    chat_id.clone(),
                    None,
                )
                .unwrap();
                assert_eq!(
                    page.turns.as_slice()[0].terminal_reason,
                    Some(crate::acp::projection::TurnTerminalReasonProjection {
                        kind: crate::acp::lifecycle::TurnTerminalReason::HarnessExited,
                        message: Some("harness process exited with code 17".to_owned()),
                        remediation: None,
                    })
                );
            }
            assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
            drop(active);
            drop(context);
            drop(sessions);
            drop(scheduler);
            drop(journal);
            journal_task.finish();
            drop(database);
            return;
        }
        let close = crate::acp::store::CloseSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision,
            client_intent_id: ClientIntentId::parse("intent-idle-close").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            now_millis: now + 4,
        };
        if case == CloseBeforeWriteCase::Prepared {
            let mut foreign = close.clone();
            foreign.runtime_epoch = RuntimeEpoch::new(2).unwrap();
            let response = mailbox
                .submit_worker(WorkerCommand::BeginClose(foreign))
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("foreign prepared close command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(
                response
                    .recv_timeout(tree_termination_deadline())
                    .expect("foreign close reply before deadline"),
                Err(HostError::Intent)
            ));
            assert!(matches!(state, FirstTurnWorkerState::Prepared { .. }));
        }
        let response = mailbox
            .submit_worker(WorkerCommand::BeginClose(close.clone()))
            .unwrap();
        let command = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .expect("begin close command");
        let ControlMessage::Worker(command) = command else {
            panic!("worker command")
        };
        state = state.handle(&context, command).await;
        match response
            .recv_timeout(tree_termination_deadline())
            .expect("close reply before deadline")
        {
            Ok(WorkerReply::Unit) => {}
            Err(error) => match &state {
                FirstTurnWorkerState::Recovery(recovery) => match recovery.as_ref() {
                    FirstTurnWorkerRecovery::BeginClose(failure) => {
                        panic!("begin close failed: {error:?}: {:?}", failure.error)
                    }
                    _ => panic!("begin close failed in another recovery: {error:?}"),
                },
                _ => panic!("begin close failed without recovery: {error:?}"),
            },
            _ => panic!("begin close reply"),
        }

        let response = mailbox
            .submit_worker(WorkerCommand::WaitZero {
                owner: owner.clone(),
            })
            .unwrap();
        let command = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .expect("wait zero command");
        let ControlMessage::Worker(command) = command else {
            panic!("worker command")
        };
        state = state.handle(&context, command).await;
        let Ok(WorkerReply::Zero(proof)) = response
            .recv_timeout(tree_termination_deadline())
            .expect("zero proof reply before deadline")
        else {
            panic!("zero proof")
        };

        let response = mailbox
            .submit_worker(WorkerCommand::FinalizeClose {
                command: close.clone(),
                proof,
            })
            .unwrap();
        let command = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .expect("finalize close command");
        let ControlMessage::Worker(command) = command else {
            panic!("worker command")
        };
        state = state.handle(&context, command).await;
        match response
            .recv_timeout(tree_termination_deadline())
            .expect("finalize close reply before deadline")
        {
            Ok(WorkerReply::Unit) => {}
            Err(error) => match &state {
                FirstTurnWorkerState::Recovery(recovery) => match recovery.as_ref() {
                    FirstTurnWorkerRecovery::Finalize(failure) => {
                        panic!("finalize close failed: {error:?}: {:?}", failure.error)
                    }
                    _ => panic!("finalize close failed in another recovery: {error:?}"),
                },
                _ => panic!("finalize close failed without recovery: {error:?}"),
            },
            _ => panic!("finalize close reply"),
        }
        assert!(matches!(state, FirstTurnWorkerState::Closed));
        assert_eq!(zero_checks.load(Ordering::SeqCst), 1);
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
        assert!(scheduler.active().is_none());
        let session: (String, String) = database
            .connection()
            .query_row(
                "SELECT state,residency FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(session, ("closed".to_owned(), "cold".to_owned()));
        let epoch_state: (String, Option<String>) = database
            .connection()
            .query_row(
                "SELECT state,exit_kind FROM session_runtime_epochs WHERE session_id=?1 AND runtime_epoch=?2",
                rusqlite::params![started.session_id.as_str(), epoch.get() as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(epoch_state, ("exited".to_owned(), Some("userClose".to_owned())));
        let counts: (i64, i64, i64, i64) = database
            .connection()
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM turns WHERE session_id=?1),
                   (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sessionCloseIntent'),
                   (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='closeFinalized'),
                   (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='turnCancelIntent')",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(counts, (i64::from(case.prepares_turn()), 1, 1, 0));
        if case.prepares_turn() {
            let turn: (String, String) = database
                .connection()
                .query_row(
                    "SELECT state,delivery_state FROM turns WHERE session_id=?1",
                    [started.session_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let expected = match case {
                CloseBeforeWriteCase::BeginSendAppliedUnknown => {
                    ("deliveryUnknown".to_owned(), "possibleBytes".to_owned())
                }
                _ => ("cancelled".to_owned(), "notWritten".to_owned()),
            };
            assert_eq!(turn, expected);
        }

        let send_started: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sendStarted'",
                [started.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            send_started,
            i64::from(case == CloseBeforeWriteCase::BeginSendAppliedUnknown)
        );

        drop(state);
        drop(context);
        drop(sessions);
        drop(scheduler);
        drop(journal);
        journal_task.finish();
        drop(database);
    });
}

#[test]
fn activation_idle_config_rejection_drains_following_frame_and_stays_promptable() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let FirstTurnFixture {
            project_id,
            chat_id,
            started,
            epoch,
            activation_journal,
            capabilities,
            external_session,
            owner,
            ..
        } = prepare_first_turn_fixture().await;
        let mut completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                ConfigThenLateAdapter { reject_next: true },
                activation_journal,
                Mapper,
                ActivatedSession {
                    binding_digest: [0x11; 32],
                    project_id,
                    chat_id,
                    session_id: started.session_id,
                    external_session_id: external_session,
                    runtime_epoch: epoch,
                    capabilities,
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
            transport: Transport(Mutex::new(VecDeque::new())),
            control: NoChildProcessControl {
                zero_checks: Arc::new(AtomicUsize::new(0)),
            },
            owner,
            clock: Arc::new(SystemDriverClock),
        };

        assert!(matches!(
            completed.accept_idle_stdout_chunk(b"{}\n{}\n"),
            Err(crate::acp::runtime::AdapterPumpError::Refused(
                crate::acp::runtime::NonfatalRefusal::ConfigMutation
            ))
        ));
        assert!(completed.idle_decoder.is_frame_aligned());
        assert!(completed.accept_idle_stdout_chunk(b"{}\n").is_ok());
        assert!(completed.idle_decoder.is_frame_aligned());
    });
}

#[test]
fn activation_idle_available_commands_update_does_not_reject_the_session() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let FirstTurnFixture {
            project_id,
            chat_id,
            started,
            epoch,
            activation_journal,
            capabilities,
            external_session,
            owner,
            ..
        } = prepare_first_turn_fixture().await;
        let mut completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                Adapter {
                    cancel_count: Arc::new(AtomicUsize::new(0)),
                    final_stdout: None,
                    config_pending: false,
                },
                activation_journal,
                Mapper,
                ActivatedSession {
                    binding_digest: [0x11; 32],
                    project_id,
                    chat_id,
                    session_id: started.session_id,
                    external_session_id: external_session.clone(),
                    runtime_epoch: epoch,
                    capabilities,
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
            transport: Transport(Mutex::new(VecDeque::new())),
            control: NoChildProcessControl {
                zero_checks: Arc::new(AtomicUsize::new(0)),
            },
            owner,
            clock: Arc::new(SystemDriverClock),
        };

        let frame = format!(
            concat!(
                r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{}","update":{{"sessionUpdate":"available_commands_update","availableCommands":[{{"name":"deep-research","description":"Deep research harness","input":null}}]}}}}}}"#,
                "\n"
            ),
            external_session.as_str()
        );
        assert!(completed.accept_idle_stdout_chunk(frame.as_bytes()).is_ok());
        assert!(completed.idle_decoder.is_frame_aligned());
    });
}

#[test]
fn idle_handoff_consumes_validated_late_stdout_before_the_first_turn() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
    let FirstTurnFixture {
        project_id,
        chat_id,
        started,
        epoch,
        activation_journal,
        capabilities,
        external_session,
        owner,
        ..
    } = prepare_first_turn_fixture().await;
    let mut completed = CompletedActivationDriverParts {
        parts: IdleActivationParts::new(
            CodexRuntime::fixture(Platform::WindowsX86_64),
            activation_journal,
            Mapper,
            ActivatedSession {
                binding_digest: [0x11; 32],
                project_id,
                chat_id,
                session_id: started.session_id,
                external_session_id: external_session,
                runtime_epoch: epoch,
                capabilities,
                models: Vec::new(),
                next_model_cursor: None,
            },
        ),
        idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
        transport: Transport(Mutex::new(VecDeque::new())),
        control: NoChildProcessControl {
            zero_checks: Arc::new(AtomicUsize::new(0)),
        },
        owner,
        clock: Arc::new(SystemDriverClock),
    };

    assert!(completed
        .accept_idle_stdout_chunk(
            br#"{"emittedAtMs":1787930996883,"method":"mcpServer/startupStatus/updated","params":{"error":null,"failureReason":null,"name":"developer","status":"ready","threadId":"f12d7062-15e0-4d7b-94a2-8a52382a4611"}}"#,
        )
        .is_ok());
    assert!(completed.accept_idle_stdout_chunk(b"\n").is_ok());
    assert!(completed
        .accept_idle_stdout_chunk(
            b"{\"emittedAtMs\":1787930996883,\"method\":\"mcpServer/startupStatus/updated\",\"params\":{\"error\":null,\"failureReason\":null,\"name\":\"developer\",\"status\":\"unknown\",\"threadId\":\"f12d7062-15e0-4d7b-94a2-8a52382a4611\"}}\n",
        )
        .is_err());
    assert!(completed.accept_idle_stdout_chunk(b"[]\n").is_err());
    });
}

fn capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: false,
        config_count: 0,
    }
}
