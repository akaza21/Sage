use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{AdapterCommand, EgressResolution, HarnessAdapter, ResourceMapper},
    contract::{
        RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
    },
    fixture::adapter::FixtureAdapter,
    intent::{ExternalId, Prompt, SafeResourceRef, UnavailableResourceReason},
    limits::LIMITS,
    process::{LaunchError, TreeExit},
    runtime::{
        ActiveTurnIntent, AdapterPump, CancelIntent, DriverClock, DriverMailbox, EgressTransport,
        JournalHandle, ProcessAdapterDriver, ProcessTreeControl, ProjectionMailbox, PumpIntent,
        SqliteAdapterBatchJournal, StdinTransport, TreeLeaseOwner,
    },
    scheduler::RuntimeOwner,
    transport::{FrameWriteOutcome, OutboundFrame},
};

use super::queue_capacity::RejectingStore;

const CANCEL_PAYLOAD: &[u8] = br#"{"jsonrpc":"2.0","method":"fixture/cancel","params":{}}"#;

pub(super) fn execute(steps: &[Value]) -> Value {
    let [fill, dispatch] = steps else {
        panic!("expected exact driver fill followed by cancel dispatch");
    };
    assert_eq!(fill["op"], "fillDriverData");
    assert_eq!(dispatch["op"], "dispatchCancel");
    let messages = field_u64(fill, "messages");
    let bytes = field_u64(fill, "bytes");
    let literal_deadline_ms = field_u64(dispatch, "deadlineMs");
    let production_deadline_ms = LIMITS.deadlines_ms.cancel_dispatch;
    assert_eq!(literal_deadline_ms, production_deadline_ms);
    assert_eq!(messages, LIMITS.queues.driver_data_messages);
    assert_eq!(bytes, messages);

    let intent = cancel_intent();
    let owner = RuntimeOwner {
        project_id: intent.project_id.clone(),
        chat_id: intent.chat_id.clone(),
        session_id: intent.session_id.clone(),
        runtime_epoch: intent.runtime_epoch,
    };
    let pump_intent = PumpIntent {
        binding_digest: [7; 32],
        project_id: intent.project_id.clone(),
        chat_id: intent.chat_id.clone(),
        session_id: intent.session_id.clone(),
        external_session_id: ExternalId::new("externalSession", "fixture-external").unwrap(),
        runtime_epoch: intent.runtime_epoch,
        active_turn: ActiveTurnIntent {
            turn_id: intent.turn_id.clone(),
            external_turn_id: None,
            client_message_id: intent.client_message_id.clone(),
        },
    };
    let mut adapter = FixtureAdapter::new(
        pump_intent.binding_digest,
        pump_intent.runtime_epoch,
        pump_intent.session_id.clone(),
        pump_intent.external_session_id.clone(),
    )
    .unwrap();
    prime_active_turn(&mut adapter, &intent.client_message_id);
    let (journal, _runner) = JournalHandle::new(RejectingStore);
    let pump = AdapterPump::new(
        adapter,
        SqliteAdapterBatchJournal::new(journal, pump_intent.clone()),
        Mapper,
        Arc::new(ProjectionMailbox::default()),
        pump_intent,
    )
    .unwrap();
    let write = Arc::new(Mutex::new(WriteObservation::default()));
    let mut driver = ProcessAdapterDriver::new(
        pump,
        RecordingTransport(Arc::clone(&write)),
        UnexpectedControl,
        Lease(owner.clone()),
        owner,
        Arc::new(FixedClock(1)),
    )
    .unwrap();
    let mailbox = DriverMailbox::new().unwrap();
    for _ in 0..messages {
        mailbox.try_push_chunk(vec![b'x']).unwrap();
    }
    let saturated_before = mailbox.try_push_chunk(vec![b'x']).is_err();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let started = Instant::now();
    mailbox
        .submit_cancel(intent, production_deadline_ms)
        .unwrap();
    let saturated_with_control = mailbox.try_push_chunk(vec![b'x']).is_err();
    let progress = runtime.block_on(driver.run_next(&mailbox)).unwrap();
    let elapsed = started.elapsed();
    let saturated_after_dispatch = mailbox.try_push_chunk(vec![b'x']).is_err();

    assert_eq!(progress.written_frames, 1);
    assert_eq!(progress.zero_byte_failures, 0);
    assert_eq!(progress.accepted_frames, 0);
    assert_eq!(progress.deferred_flushes, 0);
    assert_eq!(progress.rejected_controls, 0);
    let observed = *write.lock().unwrap();
    assert_eq!(observed.count, 1);
    assert!(observed.exact_payload);
    assert_eq!(observed.wire_bytes, CANCEL_PAYLOAD.len() + 1);
    assert_eq!(observed.outcome, Some(FrameWriteOutcome::Complete));
    assert_eq!(observed.closes, 0);
    assert!(elapsed <= Duration::from_millis(literal_deadline_ms));

    json!({
        "cancelDispatchMsMaximum": production_deadline_ms,
        "controlCapacityBorrowed": !(
            saturated_before && saturated_with_control && saturated_after_dispatch
        ),
    })
}

fn prime_active_turn(adapter: &mut FixtureAdapter, client_message_id: &RequiredText<512>) {
    let prompt = adapter
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "fixture cancel setup").unwrap(),
            client_message_id: client_message_id.clone(),
        })
        .unwrap();
    let token = prompt.into_token();
    adapter
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
}

fn cancel_intent() -> CancelIntent {
    CancelIntent {
        project_id: SageProjectId::parse("fixture-project").unwrap(),
        chat_id: SageChatId::parse("fixture-chat").unwrap(),
        session_id: SageAgentSessionId::parse("fixture-session").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: SageTurnId::parse("fixture-turn").unwrap(),
        client_message_id: RequiredText::new("clientMessageId", "fixture-turn").unwrap(),
    }
}

fn field_u64(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("missing numeric field {field}"))
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

#[derive(Clone, Copy, Default)]
struct WriteObservation {
    count: usize,
    exact_payload: bool,
    wire_bytes: usize,
    outcome: Option<FrameWriteOutcome>,
    closes: usize,
}

struct RecordingTransport(Arc<Mutex<WriteObservation>>);

impl EgressTransport for RecordingTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        let mut observed = self.0.lock().unwrap();
        observed.count = observed
            .count
            .checked_add(1)
            .expect("bounded cancel writes");
        observed.exact_payload = frame.payload() == CANCEL_PAYLOAD;
        observed.wire_bytes = frame.wire_len();
        observed.outcome = Some(FrameWriteOutcome::Complete);
        FrameWriteOutcome::Complete
    }
}

impl StdinTransport for RecordingTransport {
    fn close(&mut self) {
        let mut observed = self.0.lock().unwrap();
        observed.closes = observed.closes.checked_add(1).expect("bounded closes");
    }
}

struct UnexpectedControl;

impl ProcessTreeControl for UnexpectedControl {
    type ZeroProof = ();

    fn request_termination(&self, _reason: TreeExit) -> Result<(), LaunchError> {
        panic!("successful cancel dispatch must not request termination")
    }

    fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        panic!("successful cancel dispatch must not wait for root exit")
    }

    fn wait_zero(&self) -> Result<(TreeExit, Self::ZeroProof), LaunchError> {
        panic!("successful cancel dispatch must not wait for zero process")
    }
}
