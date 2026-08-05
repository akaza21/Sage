use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::ResourceMapper,
    contract::RequiredText,
    fixture::{generated_cases::FrozenCase, wire},
    intent::{ExternalId, SafeResourceRef, UnavailableResourceReason},
    process::{LaunchError, TreeExit},
    runtime::{
        ActiveTurnIntent, AdapterPump, DriverClock, DriverError, DriverMailbox, EgressTransport,
        JournalHandle, ProcessAdapterDriver, ProcessTreeControl, ProjectionMailbox, PumpIntent,
        SqliteAdapterBatchJournal, StdinTransport, TeardownCause, TreeLeaseOwner,
    },
    scheduler::RuntimeOwner,
    transport::{FrameWriteOutcome, OutboundFrame},
};

use super::{
    permission_fixture_adapter::PermissionFixtureAdapter,
    permission_store_fixture::{BINDING_DIGEST, PermissionStoreFixture},
};

pub(crate) const CASE_IDS: &[&str] = &[
    "permission-conflicting-duplicate",
    "permission-old-epoch-after-restart",
    "permission-payload-bytes-65536",
    "permission-payload-bytes-65537",
];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    assert!(CASE_IDS.contains(&case.id));
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(observe(case))
}

async fn observe(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let (stdout, _) = wire::capture_steps(&steps).unwrap();
    let fixture = PermissionStoreFixture::new();
    let owner = owner(&fixture);
    let writes = Arc::new(Mutex::new(EgressObservation::default()));
    let terminations = Arc::new(Mutex::new(TerminationObservation::default()));
    let transport = MemoryTransport {
        writes: Arc::clone(&writes),
    };
    let control = RecordingControl {
        terminations: Arc::clone(&terminations),
    };
    let (journal, runner) = JournalHandle::new(fixture.database_store());
    let journal_task = runner.spawn();
    let intent = pump_intent(&fixture);
    let adapter = PermissionFixtureAdapter::new(
        BINDING_DIGEST,
        fixture.runtime_epoch(),
        fixture.session_id(),
        intent.active_turn.client_message_id.clone(),
    );
    let pump = AdapterPump::new(
        adapter,
        SqliteAdapterBatchJournal::new(journal.clone(), intent),
        Mapper,
        Arc::new(ProjectionMailbox::default()),
        pump_intent(&fixture),
    )
    .unwrap();
    let mut driver = ProcessAdapterDriver::new(
        pump,
        transport,
        control,
        Lease(owner.clone()),
        owner,
        Arc::new(FixedClock(10)),
    )
    .unwrap();
    let mailbox = DriverMailbox::new().unwrap();
    let mut failure = drive_stdout(&mut driver, &mailbox, &stdout).await;
    if case.id == "permission-old-epoch-after-restart" && failure.is_none() {
        mailbox.signal_process_exit().unwrap();
        failure = driver.run_next(&mailbox).await.err();
    }
    let zero_process = failure.map(|error| {
        let handoff = driver
            .take_zero_process_handoff()
            .expect("fixture failure must converge to an exact zero-process proof");
        ZeroProcessObservation {
            error,
            cause: handoff.cause,
            exit: handoff.exit,
        }
    });
    journal.close_when_drained();
    journal_task.await.unwrap();
    let needs_recovery = matches!(
        case.id,
        "permission-conflicting-duplicate"
            | "permission-old-epoch-after-restart"
            | "permission-payload-bytes-65537"
    );
    let recovery = needs_recovery.then(|| fixture.recover(20));
    let replay =
        (case.id == "permission-old-epoch-after-restart").then(|| fixture.probe_decline_replay(21));
    let egress = *writes.lock().unwrap();
    let permission_granted = egress.grants != 0;

    match case.id {
        "permission-conflicting-duplicate" => json!({
            "terminal": fixture.session_state(),
            "permissionGranted": permission_granted,
            "treeMustExit": zero_process.as_ref().is_some_and(|observed|
                    observed.error == DriverError::TeardownRequired(
                        TeardownCause::MandatoryDenyFailure)
                    && observed.cause == TeardownCause::MandatoryDenyFailure
                    && observed.exit == TreeExit::ProtocolFailure)
                && termination_exact(&terminations, TreeExit::ProtocolFailure)
                && recovery.is_some_and(|report| report.recovered_sessions == 1)
                && fixture.request_count() == 1
                && fixture.request_state().as_deref() == Some("resolvedDeclined")
                && fixture.decline_fact_count() == 1
                && fixture.runtime_epoch_state() == "failed"
                && fixture.live_head_count() == 0
                && egress.exactly_one_decline(),
        }),
        "permission-old-epoch-after-restart" => json!({
            "session": fixture.session_state(),
            "requestReplayed": !(zero_process.as_ref().is_some_and(|observed|
                    observed.error == DriverError::TeardownRequired(TeardownCause::ProcessExit)
                    && observed.cause == TeardownCause::ProcessExit
                    && observed.exit == (TreeExit::Exited { code: None }))
                && termination_exact(&terminations, TreeExit::Exited { code: None })
                && recovery.is_some_and(|report| report.recovered_sessions == 1)
                && replay.as_ref().is_some_and(|probe| probe.duplicate && probe.writes == 0)
                && fixture.request_count() == 1
                && fixture.request_state().as_deref() == Some("resolvedDeclined")
                && fixture.decline_fact_count() == 1
                && fixture.runtime_epoch_state() == "failed"
                && fixture.live_head_count() == 0
                && egress.exactly_one_decline()),
            "permissionGranted": permission_granted,
        }),
        "permission-payload-bytes-65536" => json!({
            "accepted": failure.is_none()
                && zero_process.is_none()
                && fixture.request_count() == 1
                && fixture.request_state().as_deref() == Some("declineCommitted")
                && fixture.decline_fact_count() == 1
                && fixture.runtime_epoch_state() == "running"
                && fixture.live_head_count() == 1
                && egress.exactly_one_decline(),
            "permissionGranted": permission_granted,
        }),
        "permission-payload-bytes-65537" => json!({
            "accepted": !(zero_process.as_ref().is_some_and(|observed|
                    observed.error == DriverError::TeardownRequired(TeardownCause::PumpFailure)
                    && observed.cause == TeardownCause::PumpFailure
                    && observed.exit == TreeExit::ProtocolFailure)
                && termination_exact(&terminations, TreeExit::ProtocolFailure)
                && recovery.is_some_and(|report| report.recovered_sessions == 1)
                && fixture.request_count() == 0
                && fixture.request_state().is_none()
                && fixture.decline_fact_count() == 0
                && fixture.runtime_epoch_state() == "failed"
                && fixture.live_head_count() == 0
                && egress.no_writes()),
            "permissionGranted": permission_granted,
            "session": fixture.session_state(),
        }),
        id => panic!("unregistered permission fixture {id}"),
    }
}

async fn drive_stdout(
    driver: &mut ProcessAdapterDriver<
        PermissionFixtureAdapter,
        SqliteAdapterBatchJournal,
        Mapper,
        MemoryTransport,
        RecordingControl,
        Lease,
    >,
    mailbox: &DriverMailbox,
    stdout: &[u8],
) -> Option<DriverError> {
    for chunk in stdout.chunks(8 * 1024) {
        mailbox.try_push_chunk(chunk.to_vec()).unwrap();
        if let Err(error) = driver.run_next(mailbox).await {
            return Some(error);
        }
    }
    None
}

fn pump_intent(fixture: &PermissionStoreFixture) -> PumpIntent {
    PumpIntent {
        binding_digest: BINDING_DIGEST,
        project_id: fixture.project_id(),
        chat_id: fixture.chat_id(),
        session_id: fixture.session_id(),
        external_session_id: ExternalId::new("externalSession", "fixture-session").unwrap(),
        runtime_epoch: fixture.runtime_epoch(),
        active_turn: ActiveTurnIntent {
            turn_id: fixture.turn_id(),
            external_turn_id: Some(ExternalId::new("externalTurn", "fixture-turn").unwrap()),
            client_message_id: RequiredText::new("message", "message-permission").unwrap(),
        },
    }
}

fn owner(fixture: &PermissionStoreFixture) -> RuntimeOwner {
    RuntimeOwner {
        project_id: fixture.project_id(),
        chat_id: fixture.chat_id(),
        session_id: fixture.session_id(),
        runtime_epoch: fixture.runtime_epoch(),
    }
}

fn termination_exact(terminations: &Mutex<TerminationObservation>, expected: TreeExit) -> bool {
    let observed = terminations.lock().unwrap();
    observed.count == 1 && observed.last == Some(expected)
}

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}

struct FixedClock(i64);

impl DriverClock for FixedClock {
    fn now_millis(&self) -> i64 {
        self.0
    }
}

struct Lease(RuntimeOwner);

impl TreeLeaseOwner for Lease {
    fn runtime_owner(&self) -> &RuntimeOwner {
        &self.0
    }
}

struct MemoryTransport {
    writes: Arc<Mutex<EgressObservation>>,
}

impl EgressTransport for MemoryTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        let mut writes = self.writes.lock().unwrap();
        writes.count = writes.count.checked_add(1).expect("bounded fixture writes");
        let parsed = serde_json::from_slice::<Value>(frame.payload());
        match parsed {
            Ok(value)
                if value.get("method").and_then(Value::as_str)
                    == Some("fixture/permissionDecline")
                    && value.pointer("/params/requestId").and_then(Value::as_str)
                        == Some("permission-a")
                    && value.pointer("/params/decision").and_then(Value::as_str)
                        == Some("declined") =>
            {
                writes.declines = writes
                    .declines
                    .checked_add(1)
                    .expect("bounded fixture declines");
            }
            Ok(value)
                if matches!(
                    value.pointer("/params/decision").and_then(Value::as_str),
                    Some("allow" | "allowed" | "granted")
                ) =>
            {
                writes.grants = writes
                    .grants
                    .checked_add(1)
                    .expect("bounded fixture grants");
            }
            _ => {
                writes.invalid = writes
                    .invalid
                    .checked_add(1)
                    .expect("bounded invalid fixture egress");
            }
        }
        FrameWriteOutcome::Complete
    }
}

impl StdinTransport for MemoryTransport {
    fn close(&mut self) {}
}

struct RecordingControl {
    terminations: Arc<Mutex<TerminationObservation>>,
}

impl ProcessTreeControl for RecordingControl {
    type ZeroProof = ();

    fn request_termination(&self, reason: TreeExit) -> Result<(), LaunchError> {
        let mut terminations = self.terminations.lock().unwrap();
        terminations.count = terminations
            .count
            .checked_add(1)
            .expect("bounded fixture terminations");
        terminations.last = Some(reason);
        Ok(())
    }

    fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        self.terminations
            .lock()
            .unwrap()
            .last
            .ok_or(LaunchError::PlatformUnavailable)
    }

    fn wait_zero(&self) -> Result<(TreeExit, Self::ZeroProof), LaunchError> {
        self.wait_root_exit().map(|exit| (exit, ()))
    }
}

#[derive(Clone, Copy, Default)]
struct EgressObservation {
    count: usize,
    declines: usize,
    grants: usize,
    invalid: usize,
}

impl EgressObservation {
    fn exactly_one_decline(self) -> bool {
        self.count == 1 && self.declines == 1 && self.grants == 0 && self.invalid == 0
    }

    fn no_writes(self) -> bool {
        self.count == 0 && self.declines == 0 && self.grants == 0 && self.invalid == 0
    }
}

#[derive(Default)]
struct TerminationObservation {
    count: usize,
    last: Option<TreeExit>,
}

struct ZeroProcessObservation {
    error: DriverError,
    cause: TeardownCause,
    exit: TreeExit,
}
