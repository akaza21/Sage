use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicI64, Ordering},
        mpsc,
    },
    thread,
};

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterIdentity,
        AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
    },
    contract::{
        ProjectionRevision, RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId, SageChatId,
        SageProjectId, SageServerRequestId, SageTurnId,
    },
    intent::{CapabilitySnapshotV1, ExternalId, NormalizedPayload, SafeResourceRef},
    lifecycle::{DeliveryState, ItemLifecycle, ServerRequestState, TurnState},
    limits::LIMITS,
    process::{LaunchError, TreeExit},
    runtime::{
        ActiveTurnIntent, AdapterBatchJournal, AdapterPump, DurableAdapterEvent, EgressTransport,
        JournalReceipt, ProjectionPublishError, ProjectionPublishStatus, ProjectionPublisher,
        PumpIntent, PumpJournalError, SessionPumpIntent, permissions::PermissionDecision,
    },
    scheduler::RuntimeOwner,
    store::SendObservation,
    transport::{Frame, FrameWriteOutcome, OutboundFrame},
};

use super::*;

#[derive(Default)]
struct SharedState {
    batches: VecDeque<AdapterBatch>,
    commands: Vec<&'static str>,
    resolutions: Vec<EgressResolution>,
    permission_states: Vec<ServerRequestState>,
    durable: Vec<Vec<DurableAdapterEvent>>,
    rebound_turns: Vec<SageTurnId>,
    ingest_error: Option<AdapterRuntimeError>,
    worker_observation: Option<Arc<DriverMailbox>>,
    worker_response: Option<mpsc::Receiver<Result<WorkerReply, crate::acp::runtime::HostError>>>,
}

#[derive(Clone)]
struct Shared(Arc<Mutex<SharedState>>);

struct FakeAdapter {
    shared: Shared,
    next_request: u64,
}

impl HarnessAdapter for FakeAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let AdapterCommand::ResolvePermission {
            request_id,
            decision,
        } = command
        {
            self.shared
                .0
                .lock()
                .expect("shared")
                .commands
                .push("permission");
            let external = ExternalId::new("permissionRequest", request_id.as_str())
                .map_err(|_| AdapterRuntimeError::Frame)?;
            let token = match decision {
                PermissionDecision::Ask => EgressToken::protocol_notification(),
                PermissionDecision::AllowOnce => EgressToken::permission_allow(external),
                PermissionDecision::Deny => EgressToken::mandatory_deny(external, false),
            };
            return Ok(AdapterEgress::new(
                OutboundFrame::new(br#"{"permission":true}"#.to_vec())
                    .map_err(|_| AdapterRuntimeError::Frame)?,
                token,
            ));
        }
        let cancel = matches!(command, AdapterCommand::Cancel);
        let prompt = matches!(command, AdapterCommand::Prompt { .. });
        self.shared
            .0
            .lock()
            .expect("shared")
            .commands
            .push(if cancel {
                "cancel"
            } else if prompt {
                "prompt"
            } else {
                "command"
            });
        self.next_request += 1;
        let token = if cancel {
            EgressToken::cancel_request(self.next_request)
        } else {
            EgressToken::request(self.next_request)
        };
        Ok(AdapterEgress::new(
            OutboundFrame::new(format!(r#"{{"id":{}}}"#, self.next_request).into_bytes())
                .expect("frame"),
            token,
        ))
    }

    fn cancel_permission(
        &mut self,
        request_id: &SageServerRequestId,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.shared
            .0
            .lock()
            .expect("shared")
            .commands
            .push("cancelPermission");
        let external = ExternalId::new("permissionRequest", request_id.as_str())
            .map_err(|_| AdapterRuntimeError::PermissionIntent)?;
        Ok(AdapterEgress::new(
            OutboundFrame::new(
                format!(
                    r#"{{"id":"{}","result":{{"outcome":{{"outcome":"cancelled"}}}}}}"#,
                    request_id.as_str()
                )
                .into_bytes(),
            )
            .map_err(|_| AdapterRuntimeError::Frame)?,
            EgressToken::permission_cancel(external),
        ))
    }

    fn ingest(
        &mut self,
        _: &Frame,
        _: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let mut state = self.shared.0.lock().expect("shared");
        if let Some(error) = state.ingest_error.take() {
            return Err(error);
        }
        let worker_observation = state.worker_observation.take();
        let batch = state
            .batches
            .pop_front()
            .ok_or(AdapterRuntimeError::Operation);
        drop(state);
        if let Some(mailbox) = worker_observation {
            let response = mailbox
                .submit_worker(WorkerCommand::ObserveSend {
                    owner: owner(),
                    turn_id: turn("one"),
                    observation: SendObservation::CompleteBytes,
                })
                .ok();
            self.shared.0.lock().expect("shared").worker_response = response;
        }
        batch
    }

    fn commit_mandatory_egress(&mut self, _: &EgressToken) -> Result<(), AdapterRuntimeError> {
        Ok(())
    }

    fn resolve_egress(
        &mut self,
        _: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.shared
            .0
            .lock()
            .expect("shared")
            .resolutions
            .push(resolution);
        Ok(())
    }
}

struct FakeJournal {
    shared: Shared,
    next_revision: u64,
    turn_id: SageTurnId,
    turn_state: TurnState,
    turn_revision: u64,
}

impl AdapterBatchJournal for FakeJournal {
    fn commit<'a>(
        &'a mut self,
        events: Vec<DurableAdapterEvent>,
        _: usize,
    ) -> Pin<Box<dyn Future<Output = Result<JournalReceipt, PumpJournalError>> + Send + 'a>> {
        self.next_revision += 1;
        for event in &events {
            if let DurableAdapterEvent::Turn { state, .. } = event
                && *state != self.turn_state
            {
                self.turn_state = *state;
                self.turn_revision += 1;
            }
        }
        let turn_cursor = crate::acp::store::AdapterTurnCursor::test_only(
            self.turn_id.clone(),
            self.turn_state,
            DeliveryState::Correlated,
            crate::acp::contract::EntityRevision::new(self.turn_revision).expect("turn revision"),
            ProjectionRevision::new(self.next_revision).expect("projection revision"),
        );
        self.shared.0.lock().expect("shared").durable.push(events);
        let projection_revision = ProjectionRevision::new(self.next_revision).expect("revision");
        Box::pin(async move { Ok(JournalReceipt::committed(projection_revision, turn_cursor)) })
    }

    fn try_rebind(mut self, intent: PumpIntent) -> Result<Self, (Self, PumpJournalError)> {
        self.shared
            .0
            .lock()
            .expect("shared")
            .rebound_turns
            .push(intent.active_turn.turn_id.clone());
        self.turn_id = intent.active_turn.turn_id;
        self.turn_state = TurnState::Sending;
        self.turn_revision = 1;
        Ok(self)
    }

    fn transition_permission<'a>(
        &'a mut self,
        _: &'a crate::acp::runtime::permissions::PermissionIntent,
        state: ServerRequestState,
        _: i64,
    ) -> Pin<Box<dyn Future<Output = Result<ProjectionRevision, PumpJournalError>> + Send + 'a>>
    {
        self.next_revision += 1;
        self.shared
            .0
            .lock()
            .expect("shared")
            .permission_states
            .push(state);
        let revision = ProjectionRevision::new(self.next_revision).expect("revision");
        Box::pin(async move { Ok(revision) })
    }
}

struct FakePublisher;

impl ProjectionPublisher for FakePublisher {
    fn publish_committed(
        &self,
        _: crate::acp::intent::ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        Ok(ProjectionPublishStatus::Published)
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

#[derive(Default)]
struct FakeTransport {
    outcomes: VecDeque<FrameWriteOutcome>,
    writes: usize,
    closes: usize,
}

impl EgressTransport for FakeTransport {
    fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
        self.writes += 1;
        self.outcomes
            .pop_front()
            .unwrap_or(FrameWriteOutcome::Complete)
    }
}

impl StdinTransport for FakeTransport {
    fn close(&mut self) {
        self.closes += 1;
    }
}

#[derive(Default)]
struct ControlState {
    requested: Vec<TreeExit>,
    zero: bool,
    waits: usize,
    observations: usize,
    wait_handle_checkpoints: Vec<&'static str>,
}

#[derive(Clone)]
struct FakeControl(Arc<Mutex<ControlState>>);

impl ProcessTreeControl for FakeControl {
    type ZeroProof = u64;

    fn request_termination(&self, reason: TreeExit) -> Result<(), LaunchError> {
        self.0.lock().expect("control").requested.push(reason);
        Ok(())
    }

    fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        Err(LaunchError::Deadline)
    }

    fn log_wait_handle_observation(&self, checkpoint: &'static str) {
        self.0
            .lock()
            .expect("control")
            .wait_handle_checkpoints
            .push(checkpoint);
    }

    fn observe_root_exit(&self) -> Result<TreeExit, LaunchError> {
        self.0.lock().expect("control").observations += 1;
        Ok(TreeExit::Exited { code: Some(0) })
    }

    fn wait_zero(&self) -> Result<(TreeExit, Self::ZeroProof), LaunchError> {
        let mut state = self.0.lock().expect("control");
        state.waits += 1;
        if state.zero {
            Ok((TreeExit::ProtocolFailure, 7))
        } else {
            Err(LaunchError::Deadline)
        }
    }
}

struct FakeLease(RuntimeOwner);

impl TreeLeaseOwner for FakeLease {
    fn runtime_owner(&self) -> &RuntimeOwner {
        &self.0
    }
}

#[derive(Default)]
struct FakeClock(AtomicI64);

impl FakeClock {
    fn set(&self, value: i64) {
        self.0.store(value, Ordering::SeqCst);
    }
}

impl DriverClock for FakeClock {
    fn now_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type TestDriver =
    ProcessAdapterDriver<FakeAdapter, FakeJournal, Mapper, FakeTransport, FakeControl, FakeLease>;

fn build_driver(
    batches: Vec<AdapterBatch>,
    outcomes: VecDeque<FrameWriteOutcome>,
) -> (TestDriver, Shared, Arc<FakeClock>, Arc<Mutex<ControlState>>) {
    let shared = Shared(Arc::new(Mutex::new(SharedState {
        batches: batches.into(),
        ..SharedState::default()
    })));
    let pump = AdapterPump::new(
        FakeAdapter {
            shared: shared.clone(),
            next_request: 0,
        },
        FakeJournal {
            shared: shared.clone(),
            next_revision: 0,
            turn_id: turn("one"),
            turn_state: TurnState::Sending,
            turn_revision: 1,
        },
        Mapper,
        Arc::new(FakePublisher),
        session_intent().bind_turn(active_turn("one", "external-turn-one", "message-one")),
    )
    .expect("pump");
    let clock = Arc::new(FakeClock::default());
    clock.set(100);
    let control = Arc::new(Mutex::new(ControlState::default()));
    let owner = owner();
    let driver = ProcessAdapterDriver::new(
        pump,
        FakeTransport {
            outcomes,
            ..FakeTransport::default()
        },
        FakeControl(Arc::clone(&control)),
        FakeLease(owner.clone()),
        owner,
        clock.clone(),
    )
    .expect("driver");
    (driver, shared, clock, control)
}

#[test]
fn fragmented_multi_frame_chunks_coalesce_and_flush_on_the_single_deadline_wake() {
    runtime().block_on(async {
        let batches = vec![
            owned(vec![item(identity_one("item-one"))]),
            owned(vec![item(identity_one("item-two"))]),
        ];
        let (mut driver, shared, clock, _) = build_driver(batches, VecDeque::new());
        let mailbox = DriverMailbox::new().expect("mailbox");
        mailbox.try_push_chunk(b"{".to_vec()).expect("fragment");
        mailbox
            .try_push_chunk(b"}\n{}\n".to_vec())
            .expect("multi frame");

        assert_eq!(
            driver
                .run_next(&mailbox)
                .await
                .expect("fragment")
                .accepted_frames,
            0
        );
        assert_eq!(
            driver
                .run_next(&mailbox)
                .await
                .expect("complete frames")
                .accepted_frames,
            2
        );
        assert!(shared.0.lock().expect("shared").durable.is_empty());
        clock.set(150);
        let progress = driver.run_next(&mailbox).await.expect("deadline flush");
        assert_eq!(progress.deferred_flushes, 1);
        assert_eq!(shared.0.lock().expect("shared").durable.len(), 1);
    });
}

#[test]
fn driver_data_message_limit_plus_one_preserves_the_additional_cancel_reserve() {
    let mailbox = DriverMailbox::new().expect("mailbox");
    let data_limit = LIMITS.queues.driver_data_messages;
    for _ in 0..data_limit {
        mailbox.try_push_chunk(vec![b'x']).expect("within bound");
    }
    assert_eq!(mailbox.try_push_chunk(vec![b'x']), Err(MailboxError::Full));
    mailbox
        .submit_cancel(cancel("one", "message-one"), 10)
        .expect("reserved cancel");
    let clock = FakeClock::default();
    assert!(matches!(
        mailbox.wait_next_until(None, &clock).expect("control pop"),
        Some(ControlMessage::Cancel(_))
    ));
}

#[test]
fn driver_data_byte_limit_accepts_exact_and_rejects_plus_one_without_mutation() {
    let exact = DriverMailbox::new().expect("mailbox");
    for _ in 0..LIMITS.queues.driver_data_messages {
        exact
            .try_push_chunk(vec![b'x'; 8 * 1024])
            .expect("exact data bytes");
    }
    assert_eq!(
        exact.retained_usage().0,
        (
            LIMITS.queues.driver_data_messages as usize,
            LIMITS.queues.driver_data_bytes as usize
        )
    );

    let plus_one = DriverMailbox::new().expect("mailbox");
    for _ in 0..LIMITS.queues.driver_data_messages - 1 {
        plus_one
            .try_push_chunk(vec![b'x'; 8 * 1024])
            .expect("within byte bound");
    }
    let before = plus_one.retained_usage();
    assert!(matches!(
        plus_one.submit_prompt(
            crate::acp::intent::Prompt::new("prompt", "p".repeat(8_192)).unwrap(),
            RequiredText::new("message", "m").unwrap(),
        ),
        Err(MailboxError::Full)
    ));
    assert_eq!(plus_one.retained_usage(), before);
    plus_one
        .submit_cancel(cancel("one", "message-one"), 10)
        .expect("additional control reserve");
}

#[test]
fn zero_bytes_remains_retryable_but_first_delivery_unknown_latches_once() {
    runtime().block_on(async {
        let outcomes = VecDeque::from([
            FrameWriteOutcome::ZeroByteFailure,
            FrameWriteOutcome::DeliveryUnknown { bytes_written: 3 },
        ]);
        let (mut driver, _, _, control) = build_driver(Vec::new(), outcomes);
        let mailbox = DriverMailbox::new().expect("mailbox");
        mailbox
            .submit_cancel(cancel("one", "message-one"), 1)
            .expect("cancel");
        let first = driver.run_next(&mailbox).await.expect("known zero");
        assert_eq!(first.zero_byte_failures, 1);
        assert_eq!(driver.first_delivery_unknown(), None);

        mailbox
            .submit_cancel(cancel("one", "message-one"), 2)
            .expect("cancel");
        assert_eq!(
            driver.run_next(&mailbox).await,
            Err(DriverError::TeardownRequired(
                TeardownCause::DeliveryUnknown
            ))
        );
        assert_eq!(
            driver.first_delivery_unknown(),
            Some(FirstDeliveryUnknown {
                source: DispatchSource::Cancel
            })
        );
        assert_eq!(
            driver.transport.writes, 2,
            "driver never replays unknown bytes"
        );
        assert_eq!(control.lock().expect("control").requested.len(), 1);
        assert_eq!(
            driver.run_next(&mailbox).await,
            Err(DriverError::TeardownRequired(
                TeardownCause::DeliveryUnknown
            ))
        );
    });
}

#[test]
fn prompt_dispatch_validates_turn_intent_and_reports_exact_write_outcome() {
    let (mut complete, shared, _, _) =
        build_driver(Vec::new(), VecDeque::from([FrameWriteOutcome::Complete]));
    let progress = complete
        .dispatch_prompt(
            RequiredText::new("prompt", "hello").expect("prompt"),
            RequiredText::new("message", "message-one").expect("message"),
        )
        .expect("written prompt");
    assert_eq!(progress.written_frames, 1);
    assert_eq!(shared.0.lock().expect("shared").commands, ["prompt"]);

    let (mut foreign, shared, _, _) = build_driver(Vec::new(), VecDeque::new());
    assert_eq!(
        foreign.dispatch_prompt(
            RequiredText::new("prompt", "hello").expect("prompt"),
            RequiredText::new("message", "foreign-message").expect("message"),
        ),
        Err(DriverError::RejectedIntent)
    );
    assert!(shared.0.lock().expect("shared").commands.is_empty());

    let (mut unknown, _, _, control) = build_driver(
        Vec::new(),
        VecDeque::from([FrameWriteOutcome::DeliveryUnknown { bytes_written: 2 }]),
    );
    assert_eq!(
        unknown.dispatch_prompt(
            RequiredText::new("prompt", "hello").expect("prompt"),
            RequiredText::new("message", "message-one").expect("message"),
        ),
        Err(DriverError::TeardownRequired(
            TeardownCause::DeliveryUnknown
        ))
    );
    assert_eq!(
        unknown.first_delivery_unknown(),
        Some(FirstDeliveryUnknown {
            source: DispatchSource::Prompt
        })
    );
    assert_eq!(control.lock().expect("control").requested.len(), 1);
}

#[test]
fn prompt_crosses_the_bounded_driver_mailbox_and_returns_exact_write_evidence() {
    runtime().block_on(async {
        let (mut driver, shared, _, _) =
            build_driver(Vec::new(), VecDeque::from([FrameWriteOutcome::Complete]));
        let mailbox = DriverMailbox::new().expect("mailbox");
        let response = mailbox
            .submit_prompt(
                RequiredText::new("prompt", "hello").expect("prompt"),
                RequiredText::new("message", "message-one").expect("message"),
            )
            .expect("bounded prompt");

        driver.run_next(&mailbox).await.expect("mailbox dispatch");
        let progress = response.recv().expect("driver response").expect("write");

        assert_eq!(progress.written_frames, 1);
        assert_eq!(shared.0.lock().expect("shared").commands, ["prompt"]);
    });
}

#[test]
fn permission_write_settles_the_durable_decision() {
    runtime().block_on(async {
        let permission = AdapterEvent::PermissionRequest {
            turn_id: identity_one("turn-envelope"),
            item_id: identity_one("permission-item"),
            external_request_id: ExternalId::new("permissionRequest", "request-allow")
                .expect("request"),
            payload: NormalizedPayload::PermissionRequest {
                category: crate::acp::intent::ToolCategory::Other,
                summary: SafeText::new("summary", "permission").expect("summary"),
            },
            request_payload_digest: [3; 32],
        };
        let (mut driver, shared, _, _) = build_driver(
            vec![owned(vec![permission])],
            VecDeque::from([FrameWriteOutcome::Complete]),
        );
        let mailbox = DriverMailbox::new().expect("mailbox");
        mailbox
            .try_push_chunk(b"{}\n".to_vec())
            .expect("permission ingress");
        driver
            .run_next(&mailbox)
            .await
            .expect("permission journaled");

        driver
            .resolve_permission(
                &SageServerRequestId::parse("request-allow").expect("request"),
                PermissionDecision::AllowOnce,
            )
            .await
            .expect("allow written");

        let state = shared.0.lock().expect("shared");
        assert_eq!(
            state.permission_states,
            [
                ServerRequestState::AllowCommitted,
                ServerRequestState::ResponseQueued,
                ServerRequestState::ResolvedAllowed,
            ]
        );
        assert!(state.commands.contains(&"permission"));
    });
}

#[test]
fn worker_control_yields_between_two_decoded_frames_before_the_second_frame() {
    runtime().block_on(async {
        let batches = vec![
            owned(vec![item(identity_one("item-one"))]),
            owned(vec![item(identity_one("item-two"))]),
        ];
        let (mut driver, shared, _, _) = build_driver(batches, VecDeque::new());
        let mailbox = Arc::new(DriverMailbox::new().expect("mailbox"));
        shared.0.lock().unwrap().worker_observation = Some(Arc::clone(&mailbox));
        mailbox
            .try_push_chunk(b"{}\n{}\n".to_vec())
            .expect("two frames");

        let yielded = driver.run_next_step(&mailbox).await.unwrap();
        let DriverStep::YieldWorker { progress, envelope } = yielded else {
            panic!("worker control must yield the live driver");
        };
        assert_eq!(progress.accepted_frames, 1);
        assert_eq!(
            shared.0.lock().unwrap().batches.len(),
            1,
            "the second decoded frame stays buffered"
        );
        envelope.respond(Ok(WorkerReply::Unit));
        shared
            .0
            .lock()
            .unwrap()
            .worker_response
            .take()
            .unwrap()
            .recv()
            .unwrap()
            .unwrap();

        let resumed = driver.resume_after_worker_control(&mailbox).await.unwrap();
        let DriverStep::Progress(progress) = resumed else {
            panic!("the same driver resumes its retained decoder");
        };
        assert_eq!(progress.accepted_frames, 1);
        assert!(shared.0.lock().unwrap().batches.is_empty());
    });
}

#[test]
fn malformed_pump_input_process_exit_and_mandatory_failure_all_request_teardown() {
    runtime().block_on(async {
        let (mut malformed, _, _, _) = build_driver(Vec::new(), VecDeque::new());
        let mailbox = DriverMailbox::new().expect("mailbox");
        mailbox.try_push_chunk(b"{]\n".to_vec()).expect("chunk");
        assert_eq!(
            malformed.run_next(&mailbox).await,
            Err(DriverError::TeardownRequired(
                TeardownCause::ProtocolFailure
            ))
        );

        let (mut exited, _shared, _, _) = build_driver(
            vec![owned(vec![item(identity_one("must-not-run"))])],
            VecDeque::new(),
        );
        let mailbox = DriverMailbox::new().expect("mailbox");
        mailbox
            .signal_process_exit(TreeExit::Exited { code: Some(17) })
            .expect("exit");
        assert_eq!(
            exited.run_next(&mailbox).await,
            Err(DriverError::TeardownRequired(TeardownCause::ProcessExit))
        );

        let (mut mandatory, shared, _, control) = build_driver(Vec::new(), VecDeque::new());
        shared.0.lock().expect("shared").ingest_error =
            Some(AdapterRuntimeError::MandatoryDenyTeardown);
        let mailbox = DriverMailbox::new().expect("mailbox");
        mailbox.try_push_chunk(b"{}\n".to_vec()).expect("data");
        assert_eq!(
            mandatory.run_next(&mailbox).await,
            Err(DriverError::TeardownRequired(
                TeardownCause::MandatoryDenyFailure
            ))
        );
        assert_eq!(
            control.lock().expect("control").requested,
            [TreeExit::ProtocolFailure]
        );
    });
}

#[test]
fn process_exit_does_not_outrank_queued_stdout() {
    let mailbox = DriverMailbox::new().expect("mailbox");
    mailbox
        .try_push_chunk(b"{\"result\":\"interrupted\"}\n".to_vec())
        .expect("chunk");
    mailbox
        .signal_process_exit(TreeExit::Exited { code: Some(17) })
        .expect("exit");
    let message = mailbox
        .wait_next_until(None, &SystemDriverClock)
        .expect("pop")
        .expect("stdout first");
    assert!(
        matches!(message, ControlMessage::StdoutChunk(_)),
        "queued stdout must commit before ProcessExit"
    );
}

#[test]
fn process_exit_does_not_starve_later_worker_control() {
    let mailbox = DriverMailbox::new().expect("mailbox");
    mailbox
        .signal_process_exit(TreeExit::Exited { code: Some(17) })
        .expect("exit");
    let _shutdown = mailbox
        .submit_worker(WorkerCommand::Shutdown)
        .expect("shutdown");
    let message = mailbox
        .wait_control_until(None, &SystemDriverClock)
        .expect("pop")
        .expect("worker first");
    assert!(
        matches!(message, ControlMessage::Worker(_)),
        "ProcessExit must not hide Shutdown on the control lane"
    );
}

#[test]
fn exact_owner_epoch_and_old_turn_cancel_are_rejected_without_touching_new_turn() {
    runtime().block_on(async {
        let (mut driver, shared, _, _) = build_driver(Vec::new(), VecDeque::new());
        let mailbox = DriverMailbox::new().expect("mailbox");
        let mut foreign = cancel("one", "message-one");
        foreign.session_id = SageAgentSessionId::parse("session-foreign").expect("foreign");
        mailbox.submit_cancel(foreign, 1).expect("foreign");
        assert_eq!(
            driver.run_next(&mailbox).await,
            Err(DriverError::IdentityMismatch)
        );
        mailbox
            .submit_cancel(cancel("foreign", "message-one"), 2)
            .expect("old turn");
        assert_eq!(
            driver.run_next(&mailbox).await,
            Err(DriverError::RejectedIntent)
        );
        assert!(shared.0.lock().expect("shared").commands.is_empty());
    });
}

#[test]
fn one_live_adapter_and_tree_are_rebound_for_two_sequential_turns() {
    runtime().block_on(async {
        let batches = vec![
            owned(vec![terminal(
                identity_one("terminal-one"),
                TurnState::Completed,
            )]),
            owned(vec![terminal(
                identity_one("old-late"),
                TurnState::Completed,
            )]),
            owned(vec![terminal(
                identity_two("terminal-two"),
                TurnState::Completed,
            )]),
        ];
        let (mut driver, shared, _, _) = build_driver(batches, VecDeque::new());
        let mailbox = DriverMailbox::new().expect("mailbox");
        mailbox.try_push_chunk(b"{}\n".to_vec()).expect("turn one");
        driver.run_next(&mailbox).await.expect("turn one terminal");
        let completed = match driver.try_complete_turn(&mailbox) {
            Ok(completed) => completed,
            Err(_) => panic!("completion handback"),
        };
        assert_eq!(completed.terminal_cursor().turn_id(), &turn("one"));
        assert_eq!(completed.terminal_cursor().state(), TurnState::Completed);
        let mut driver =
            match completed.bind_turn(active_turn("two", "external-turn-two", "message-two")) {
                Ok(driver) => driver,
                Err(_) => panic!("same adapter rebound"),
            };

        mailbox
            .submit_cancel(cancel("one", "message-one"), 1)
            .expect("old cancel");
        assert_eq!(
            driver.run_next(&mailbox).await,
            Err(DriverError::RejectedIntent)
        );
        mailbox
            .try_push_chunk(b"{}\n{}\n".to_vec())
            .expect("late old plus exact new");
        driver.run_next(&mailbox).await.expect("turn two terminal");

        let state = shared.0.lock().expect("shared");
        assert_eq!(state.rebound_turns, [turn("two")]);
        assert_eq!(state.commands.len(), 0, "old cancel never reached adapter");
        assert_eq!(state.durable.len(), 2);
        let scopes = state
            .durable
            .iter()
            .flat_map(|batch| batch.iter())
            .filter_map(|event| match event {
                DurableAdapterEvent::Turn { scope, .. } => Some(scope.turn_id.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(scopes, [turn("one"), turn("two")]);
    });
}

#[test]
fn zero_process_handoff_never_releases_without_convergence_proof() {
    runtime().block_on(async {
        let (mut driver, _, _, control) = build_driver(Vec::new(), VecDeque::new());
        let mailbox = DriverMailbox::new().expect("mailbox");
        mailbox
            .signal_process_exit(TreeExit::Exited { code: Some(17) })
            .expect("exit");
        let _ = driver.run_next(&mailbox).await;
        assert!(matches!(
            driver.take_zero_process_handoff(),
            Err(DriverError::Tree)
        ));
        assert_eq!(control.lock().expect("control").waits, 1);
        control.lock().expect("control").zero = true;
        let handoff = driver
            .take_zero_process_handoff()
            .expect("exact zero proof");
        assert_eq!(handoff.proof, 7);
        assert_eq!(handoff.lease.runtime_owner(), &owner());
        assert!(matches!(
            driver.take_zero_process_handoff(),
            Err(DriverError::Tree)
        ));
    });
}

struct ScriptStdout {
    chunks: VecDeque<Result<Vec<u8>, LaunchError>>,
}

impl BlockingStdout for ScriptStdout {
    fn read_chunk(&mut self, target: &mut [u8]) -> Result<usize, LaunchError> {
        match self.chunks.pop_front() {
            Some(Ok(chunk)) => {
                target[..chunk.len()].copy_from_slice(&chunk);
                Ok(chunk.len())
            }
            Some(Err(error)) => Err(error),
            None => Ok(0),
        }
    }
}

#[test]
fn sole_blocking_reader_preserves_raw_chunk_order_before_eof() {
    let mailbox = DriverMailbox::new().expect("mailbox");
    BlockingStdoutReader::new(ScriptStdout {
        chunks: VecDeque::from([Ok(b"fir".to_vec()), Ok(b"st\nsecond\n".to_vec())]),
    })
    .run(&mailbox);
    let clock = FakeClock::default();
    for expected in [b"fir".as_slice(), b"st\nsecond\n".as_slice()] {
        match mailbox
            .wait_next_until(None, &clock)
            .expect("reader message")
        {
            Some(ControlMessage::StdoutChunk(chunk)) => assert_eq!(chunk, expected),
            _ => panic!("expected ordered raw chunk"),
        }
    }
    assert!(matches!(
        mailbox
            .wait_next_until(None, &clock)
            .expect("reader terminal"),
        Some(ControlMessage::ReaderTerminal(ReaderTerminal::Eof))
    ));
}

#[test]
fn passive_root_exit_watcher_signals_without_consuming_zero_proof() {
    let mailbox = DriverMailbox::new().expect("mailbox");
    let control = Arc::new(Mutex::new(ControlState {
        zero: true,
        ..ControlState::default()
    }));
    ProcessExitWatcher::new(FakeControl(Arc::clone(&control))).run(&mailbox);
    assert!(matches!(
        mailbox
            .wait_next_until(None, &FakeClock::default())
            .expect("process exit"),
        Some(ControlMessage::ProcessExit(TreeExit::Exited {
            code: Some(0)
        }))
    ));
    assert_eq!(
        control.lock().expect("control").waits,
        0,
        "root observation must not consume zero-process convergence"
    );
    assert_eq!(control.lock().expect("control").observations, 1);
    assert_eq!(
        control.lock().expect("control").wait_handle_checkpoints,
        ["acp.process_exit_watcher.attached"],
        "the watcher must not re-query a handle after root exit"
    );
}

struct BlockingInput {
    released: Arc<(Mutex<bool>, Condvar)>,
}

impl BlockingStdout for BlockingInput {
    fn read_chunk(&mut self, _: &mut [u8]) -> Result<usize, LaunchError> {
        let (lock, ready) = &*self.released;
        let mut released = lock.lock().expect("release");
        while !*released {
            released = ready.wait(released).expect("wait");
        }
        Ok(0)
    }
}

#[test]
fn blocked_native_read_does_not_block_cancel_control_submission() {
    let mailbox = Arc::new(DriverMailbox::new().expect("mailbox"));
    let released = Arc::new((Mutex::new(false), Condvar::new()));
    let reader_mailbox = Arc::clone(&mailbox);
    let reader_released = Arc::clone(&released);
    let reader = thread::spawn(move || {
        BlockingStdoutReader::new(BlockingInput {
            released: reader_released,
        })
        .run(&reader_mailbox);
    });
    mailbox
        .submit_cancel(cancel("one", "message-one"), 1)
        .expect("cancel does not wait for read");
    assert!(matches!(
        mailbox
            .wait_next_until(None, &FakeClock::default())
            .expect("cancel"),
        Some(ControlMessage::Cancel(_))
    ));
    let (lock, ready) = &*released;
    *lock.lock().expect("release") = true;
    ready.notify_one();
    reader.join().expect("reader join");
}

fn owned(events: Vec<AdapterEvent>) -> AdapterBatch {
    AdapterBatch::new(events, Vec::new()).expect("batch")
}

fn item(id: AdapterIdentity) -> AdapterEvent {
    AdapterEvent::Item {
        id,
        lifecycle: ItemLifecycle::Streaming,
        tool_state: None,
        payload: NormalizedPayload::Text {
            text: SafeText::new("text", "bounded").expect("text"),
        },
    }
}

fn terminal(id: AdapterIdentity, state: TurnState) -> AdapterEvent {
    AdapterEvent::Turn { id, state }
}

fn identity_one(external: &str) -> AdapterIdentity {
    identity(external, "external-turn-one", "message-one")
}

fn identity_two(external: &str) -> AdapterIdentity {
    identity(external, "external-turn-two", "message-two")
}

fn identity(external: &str, external_turn: &str, message: &str) -> AdapterIdentity {
    AdapterIdentity {
        binding_digest: [7; 32],
        runtime_epoch: RuntimeEpoch::new(7).expect("epoch"),
        session_id: session(),
        external_id: ExternalId::new("external", external).expect("external"),
        turn_id: Some(ExternalId::new("turn", external_turn).expect("turn")),
        turn_client_message_id: Some(RequiredText::new("message", message).expect("message")),
    }
}

fn owner() -> RuntimeOwner {
    RuntimeOwner {
        project_id: project(),
        chat_id: chat(),
        session_id: session(),
        runtime_epoch: RuntimeEpoch::new(7).expect("epoch"),
    }
}

fn session_intent() -> SessionPumpIntent {
    SessionPumpIntent {
        binding_digest: [7; 32],
        project_id: project(),
        chat_id: chat(),
        session_id: session(),
        external_session_id: ExternalId::new("session", "external-session").expect("session"),
        runtime_epoch: RuntimeEpoch::new(7).expect("epoch"),
    }
}

fn active_turn(id: &str, external: &str, message: &str) -> ActiveTurnIntent {
    ActiveTurnIntent {
        turn_id: turn(id),
        external_turn_id: Some(ExternalId::new("externalTurn", external).expect("external turn")),
        client_message_id: RequiredText::new("message", message).expect("message"),
    }
}

fn cancel(turn_id: &str, message: &str) -> CancelIntent {
    CancelIntent {
        project_id: project(),
        chat_id: chat(),
        session_id: session(),
        runtime_epoch: RuntimeEpoch::new(7).expect("epoch"),
        turn_id: turn(turn_id),
        client_message_id: RequiredText::new("message", message).expect("message"),
    }
}

fn project() -> SageProjectId {
    SageProjectId::parse("project-one").expect("project")
}

fn chat() -> SageChatId {
    SageChatId::parse("chat-one").expect("chat")
}

fn session() -> SageAgentSessionId {
    SageAgentSessionId::parse("session-one").expect("session")
}

fn turn(value: &str) -> SageTurnId {
    SageTurnId::parse(format!("turn-{value}")).expect("turn")
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

#[path = "cancel_permission_tests.rs"]
mod cancel_permission_tests;
