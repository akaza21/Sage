use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};

use crate::acp::{
    bindings::adapter::{
        AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterIdentity,
        AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
    },
    process::TreeExit,
    runtime::{
        ActivationIntent, ActivationJournal, ActivationJournalError, ActivationMode,
        ActivationPump, ActivationReceipt, ActivationTransport, EgressTransport,
    },
    store::CompleteActivationCommand,
    transport::{Frame, FrameWriteOutcome, OutboundFrame},
};
use sage_acp_protocol::{
    contract::{
        ProjectionRevision, RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId,
        SageProjectId, SchemaVersionV1,
    },
    intent::{CapabilitySnapshotV1, ExternalId, SafeResourceRef},
    scheduler::RuntimeOwner,
};
use sage_core::limits::LIMITS;
use sage_process::error::LaunchError;

use super::*;

struct Adapter {
    batches: VecDeque<AdapterBatch>,
    next: u64,
}

impl HarnessAdapter for Adapter {
    fn goal_get(&mut self) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        Ok(Some(AdapterEgress::new(
            OutboundFrame::new(br#"{"id":999,"method":"thread/goal/get"}"#.to_vec()).unwrap(),
            EgressToken::request(999),
        )))
    }

    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        Some(capabilities())
    }

    fn command(&mut self, _: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.next += 1;
        Ok(AdapterEgress::new(
            OutboundFrame::new(format!("{{\"id\":{}}}", self.next).into_bytes()).unwrap(),
            EgressToken::request(self.next),
        ))
    }

    fn ingest(
        &mut self,
        _: &Frame,
        _: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        self.batches
            .pop_front()
            .ok_or(AdapterRuntimeError::Operation)
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

struct Journal(Arc<Mutex<usize>>);

impl ActivationJournal for Journal {
    fn commit_ready<'a>(
        &'a mut self,
        _: CompleteActivationCommand,
    ) -> Pin<Box<dyn Future<Output = Result<ActivationReceipt, ActivationJournalError>> + Send + 'a>>
    {
        *self.0.lock().unwrap() += 1;
        Box::pin(async {
            Ok(ActivationReceipt {
                projection_revision: ProjectionRevision::new(4).unwrap(),
            })
        })
    }
}

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<SafeResourceRef, sage_acp_protocol::intent::UnavailableResourceReason> {
        Err(sage_acp_protocol::intent::UnavailableResourceReason::Unsupported)
    }
}

struct Transport {
    outcomes: VecDeque<FrameWriteOutcome>,
    writes: usize,
    closes: usize,
}

impl EgressTransport for Transport {
    fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
        self.writes += 1;
        self.outcomes
            .pop_front()
            .unwrap_or(FrameWriteOutcome::Complete)
    }
}

impl ActivationTransport for Transport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        EgressTransport::write(self, frame)
    }
}

impl StdinTransport for Transport {
    fn close(&mut self) {
        self.closes += 1;
    }
}

#[derive(Clone)]
struct Control(Arc<Mutex<ControlState>>);

#[derive(Default)]
struct ControlState {
    requested: usize,
    requested_exits: Vec<TreeExit>,
    zero: bool,
}

impl ProcessTreeControl for Control {
    type ZeroProof = u64;

    fn request_termination(&self, exit: TreeExit) -> Result<(), LaunchError> {
        let mut state = self.0.lock().unwrap();
        state.requested += 1;
        state.requested_exits.push(exit);
        Ok(())
    }

    fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        Ok(TreeExit::Exited { code: Some(0) })
    }

    fn wait_zero(&self) -> Result<(TreeExit, Self::ZeroProof), LaunchError> {
        if self.0.lock().unwrap().zero {
            Ok((TreeExit::ProtocolFailure, 9))
        } else {
            Err(LaunchError::Deadline)
        }
    }
}

struct Clock;

impl DriverClock for Clock {
    fn now_millis(&self) -> i64 {
        10
    }
}

struct MutableClock(AtomicI64);

impl MutableClock {
    fn new(now_millis: i64) -> Self {
        Self(AtomicI64::new(now_millis))
    }

    fn set(&self, now_millis: i64) {
        self.0.store(now_millis, Ordering::SeqCst);
    }
}

impl DriverClock for MutableClock {
    fn now_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[test]
fn activation_driver_hands_off_goal_hydration_only_after_ready_and_quiescent() {
    runtime().block_on(async {
        let commits = Arc::new(Mutex::new(0));
        let pump = ActivationPump::new(
            Adapter {
                batches: VecDeque::from([
                    AdapterBatch::new(
                        vec![AdapterEvent::Initialized {
                            capabilities: capabilities(),
                        }],
                        vec![],
                    )
                    .unwrap(),
                    AdapterBatch::new(
                        vec![AdapterEvent::SessionReady {
                            id: identity(),
                            loaded: false,
                        }],
                        vec![],
                    )
                    .unwrap(),
                ]),
                next: 0,
            },
            Journal(Arc::clone(&commits)),
            Mapper,
            intent(),
        )
        .unwrap();
        let control = Arc::new(Mutex::new(ControlState::default()));
        let mut driver = ActivationProcessDriver::new(
            pump,
            Transport {
                outcomes: VecDeque::new(),
                writes: 0,
                closes: 0,
            },
            Control(Arc::clone(&control)),
            owner(),
            Arc::new(Clock),
        )
        .unwrap();
        assert_eq!(driver.begin().unwrap().written_frames, 1);
        let mailbox = DriverMailbox::new().unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        assert_eq!(driver.run_next(&mailbox).await.unwrap().written_frames, 1);
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        driver.run_next(&mailbox).await.unwrap();
        assert!(driver.completion_ready(&mailbox));
        let mut parts = match driver.try_into_idle_parts(&mailbox) {
            Ok(parts) => parts,
            Err(_) => panic!("ready handoff"),
        };
        let activation_writes = parts.transport.writes;
        parts.parts.hydrate_goal(&mut parts.transport);
        assert_eq!(parts.transport.writes, activation_writes + 1);
        assert_eq!(parts.owner, owner());
        assert_eq!(*commits.lock().unwrap(), 1);
        assert_eq!(
            parts.parts.session().external_session_id.as_str(),
            "external"
        );
        assert_eq!(control.lock().unwrap().requested, 0);
    });
}

#[test]
fn activation_process_exit_preserves_its_observed_code_after_delayed_stdout() {
    runtime().block_on(async {
        let commits = Arc::new(Mutex::new(0));
        let pump = ActivationPump::new(
            Adapter {
                batches: VecDeque::from([
                    AdapterBatch::new(
                        vec![AdapterEvent::Initialized {
                            capabilities: capabilities(),
                        }],
                        vec![],
                    )
                    .unwrap(),
                    AdapterBatch::new(
                        vec![AdapterEvent::SessionReady {
                            id: identity(),
                            loaded: false,
                        }],
                        vec![],
                    )
                    .unwrap(),
                ]),
                next: 0,
            },
            Journal(Arc::clone(&commits)),
            Mapper,
            intent(),
        )
        .unwrap();
        let control = Arc::new(Mutex::new(ControlState::default()));
        let mut driver = ActivationProcessDriver::new(
            pump,
            Transport {
                outcomes: VecDeque::new(),
                writes: 0,
                closes: 0,
            },
            Control(Arc::clone(&control)),
            owner(),
            Arc::new(Clock),
        )
        .unwrap();
        let mailbox = DriverMailbox::new().unwrap();

        driver.begin().unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        driver.run_next(&mailbox).await.unwrap();
        mailbox
            .signal_process_exit(TreeExit::Exited { code: Some(17) })
            .unwrap();
        driver.run_next(&mailbox).await.unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        driver.run_next(&mailbox).await.unwrap();
        mailbox.signal_reader_terminal(ReaderTerminal::Eof).unwrap();

        assert_eq!(
            driver.run_next(&mailbox).await,
            Err(ActivationDriverError::TeardownRequired(
                TeardownCause::ProcessExit
            ))
        );
        assert_eq!(*commits.lock().unwrap(), 1);
        assert_eq!(
            control.lock().unwrap().requested_exits,
            [TreeExit::Exited { code: Some(17) }]
        );
    });
}

#[test]
fn activation_reader_eof_before_late_process_exit_preserves_the_exit_code() {
    runtime().block_on(async {
        let pump = ActivationPump::new(
            Adapter {
                batches: VecDeque::new(),
                next: 0,
            },
            Journal(Arc::new(Mutex::new(0))),
            Mapper,
            intent(),
        )
        .unwrap();
        let control = Arc::new(Mutex::new(ControlState::default()));
        let mut driver = ActivationProcessDriver::new(
            pump,
            Transport {
                outcomes: VecDeque::new(),
                writes: 0,
                closes: 0,
            },
            Control(Arc::clone(&control)),
            owner(),
            Arc::new(Clock),
        )
        .unwrap();
        let mailbox = DriverMailbox::new().unwrap();

        mailbox.signal_reader_terminal(ReaderTerminal::Eof).unwrap();
        assert_eq!(
            driver.run_next(&mailbox).await,
            Ok(DriverProgress::default())
        );
        mailbox
            .signal_process_exit(TreeExit::Exited { code: Some(17) })
            .unwrap();

        assert_eq!(
            driver.run_next(&mailbox).await,
            Err(ActivationDriverError::TeardownRequired(
                TeardownCause::ProcessExit
            ))
        );
        assert_eq!(
            control.lock().unwrap().requested_exits,
            [TreeExit::Exited { code: Some(17) }]
        );
    });
}

#[test]
fn activation_process_exit_without_eof_latches_at_its_deadline() {
    runtime().block_on(async {
        let pump = ActivationPump::new(
            Adapter {
                batches: VecDeque::new(),
                next: 0,
            },
            Journal(Arc::new(Mutex::new(0))),
            Mapper,
            intent(),
        )
        .unwrap();
        let clock = Arc::new(MutableClock::new(10));
        let mut driver = ActivationProcessDriver::new(
            pump,
            Transport {
                outcomes: VecDeque::new(),
                writes: 0,
                closes: 0,
            },
            Control(Arc::new(Mutex::new(ControlState::default()))),
            owner(),
            Arc::clone(&clock) as Arc<dyn DriverClock>,
        )
        .unwrap();
        let mailbox = DriverMailbox::new().unwrap();
        mailbox
            .signal_process_exit(TreeExit::Exited { code: Some(17) })
            .unwrap();
        driver.run_next(&mailbox).await.unwrap();
        clock.set(10 + LIMITS.deadlines_ms.tree_termination as i64);

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            driver.run_next(&mailbox),
        )
        .await;
        assert_eq!(
            result,
            Ok(Err(ActivationDriverError::TeardownRequired(
                TeardownCause::ProcessExit
            )))
        );
    });
}

#[test]
fn activation_driver_hands_off_when_session_ready_and_available_commands_share_a_chunk() {
    runtime().block_on(async {
        let commits = Arc::new(Mutex::new(0));
        let pump = ActivationPump::new(
            Adapter {
                batches: VecDeque::from([
                    AdapterBatch::new(
                        vec![AdapterEvent::Initialized {
                            capabilities: capabilities(),
                        }],
                        vec![],
                    )
                    .unwrap(),
                    AdapterBatch::new(
                        vec![AdapterEvent::SessionReady {
                            id: identity(),
                            loaded: false,
                        }],
                        vec![],
                    )
                    .unwrap(),
                ]),
                next: 0,
            },
            Journal(Arc::clone(&commits)),
            Mapper,
            intent(),
        )
        .unwrap();
        let mut driver = ActivationProcessDriver::new(
            pump,
            Transport {
                outcomes: VecDeque::new(),
                writes: 0,
                closes: 0,
            },
            Control(Arc::new(Mutex::new(ControlState::default()))),
            owner(),
            Arc::new(Clock),
        )
        .unwrap();
        let mailbox = DriverMailbox::new().unwrap();
        driver.begin().unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        driver.run_next(&mailbox).await.unwrap();
        let session_and_update = format!(
            concat!(
                "{{}}\n",
                r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"{}","update":{{"sessionUpdate":"available_commands_update","availableCommands":[]}}}}}}"#,
                "\n",
            ),
            identity().external_id.as_str(),
        );
        mailbox
            .try_push_chunk(session_and_update.into_bytes())
            .unwrap();
        driver.run_next(&mailbox).await.unwrap();

        assert!(driver.completion_ready(&mailbox));
        assert!(driver.try_into_idle_parts(&mailbox).is_ok());
        assert_eq!(*commits.lock().unwrap(), 1);
    });
}

#[test]
fn activation_driver_preserves_ready_handoff_when_a_late_frame_arrives() {
    runtime().block_on(async {
        let commits = Arc::new(Mutex::new(0));
        let pump = ActivationPump::new(
            Adapter {
                batches: VecDeque::from([
                    AdapterBatch::new(
                        vec![AdapterEvent::Initialized {
                            capabilities: capabilities(),
                        }],
                        vec![],
                    )
                    .unwrap(),
                    AdapterBatch::new(
                        vec![AdapterEvent::SessionReady {
                            id: identity(),
                            loaded: false,
                        }],
                        vec![],
                    )
                    .unwrap(),
                    AdapterBatch::new(vec![AdapterEvent::IgnoredLate], vec![]).unwrap(),
                ]),
                next: 0,
            },
            Journal(commits),
            Mapper,
            intent(),
        )
        .unwrap();
        let mut driver = ActivationProcessDriver::new(
            pump,
            Transport {
                outcomes: VecDeque::new(),
                writes: 0,
                closes: 0,
            },
            Control(Arc::new(Mutex::new(ControlState::default()))),
            owner(),
            Arc::new(Clock),
        )
        .unwrap();
        let mailbox = DriverMailbox::new().unwrap();
        driver.begin().unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        driver.run_next(&mailbox).await.unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        driver.run_next(&mailbox).await.unwrap();
        assert!(driver.completion_ready(&mailbox));

        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        let failure = match driver.try_into_idle_parts(&mailbox) {
            Ok(_) => panic!("late frame must block the ready handoff"),
            Err(failure) => failure,
        };
        let mut driver = failure.driver;
        driver.run_next(&mailbox).await.unwrap();

        assert!(driver.try_into_idle_parts(&mailbox).is_ok());
    });
}

#[test]
fn activation_driver_hands_off_a_queued_first_turn_command() {
    runtime().block_on(async {
        let pump = ActivationPump::new(
            Adapter {
                batches: VecDeque::from([
                    AdapterBatch::new(
                        vec![AdapterEvent::Initialized {
                            capabilities: capabilities(),
                        }],
                        vec![],
                    )
                    .unwrap(),
                    AdapterBatch::new(
                        vec![AdapterEvent::SessionReady {
                            id: identity(),
                            loaded: false,
                        }],
                        vec![],
                    )
                    .unwrap(),
                ]),
                next: 0,
            },
            Journal(Arc::new(Mutex::new(0))),
            Mapper,
            intent(),
        )
        .unwrap();
        let mut driver = ActivationProcessDriver::new(
            pump,
            Transport {
                outcomes: VecDeque::new(),
                writes: 0,
                closes: 0,
            },
            Control(Arc::new(Mutex::new(ControlState::default()))),
            owner(),
            Arc::new(Clock),
        )
        .unwrap();
        let mailbox = DriverMailbox::new().unwrap();
        driver.begin().unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        driver.run_next(&mailbox).await.unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        driver.run_next(&mailbox).await.unwrap();
        mailbox
            .submit_worker(WorkerCommand::AcceptLatestCompleted)
            .unwrap();

        let _parts = match driver.try_into_idle_parts(&mailbox) {
            Ok(parts) => parts,
            Err(_) => panic!("first turn command must cross the activation handoff"),
        };
        assert!(matches!(
            mailbox.wait_next_until(None, &Clock).unwrap(),
            Some(ControlMessage::Worker(_))
        ));
    });
}

#[test]
fn activation_delivery_unknown_never_replays_and_requires_zero_proof() {
    let pump = ActivationPump::new(
        Adapter {
            batches: VecDeque::new(),
            next: 0,
        },
        Journal(Arc::new(Mutex::new(0))),
        Mapper,
        intent(),
    )
    .unwrap();
    let control = Arc::new(Mutex::new(ControlState::default()));
    let mut driver = ActivationProcessDriver::new(
        pump,
        Transport {
            outcomes: VecDeque::from([FrameWriteOutcome::DeliveryUnknown { bytes_written: 1 }]),
            writes: 0,
            closes: 0,
        },
        Control(Arc::clone(&control)),
        owner(),
        Arc::new(Clock),
    )
    .unwrap();
    assert_eq!(
        driver.begin(),
        Err(ActivationDriverError::TeardownRequired(
            TeardownCause::DeliveryUnknown
        ))
    );
    let failure = match driver.take_zero_process_handoff() {
        Ok(_) => panic!("zero not proven"),
        Err(failure) => failure,
    };
    control.lock().unwrap().zero = true;
    let handoff = match failure.driver.take_zero_process_handoff() {
        Ok(handoff) => handoff,
        Err(_) => panic!("exact zero handoff"),
    };
    assert_eq!(handoff.proof, 9);
    assert_eq!(
        handoff.first_delivery_unknown,
        Some(FirstDeliveryUnknown {
            source: DispatchSource::Activation,
        })
    );
    assert_eq!(control.lock().unwrap().requested, 1);
}

#[test]
fn constructor_identity_failure_retains_all_activation_parts() {
    let pump = ActivationPump::new(
        Adapter {
            batches: VecDeque::new(),
            next: 0,
        },
        Journal(Arc::new(Mutex::new(0))),
        Mapper,
        intent(),
    )
    .unwrap();
    let foreign = RuntimeOwner {
        runtime_epoch: RuntimeEpoch::new(4).unwrap(),
        ..owner()
    };
    let failure = match ActivationProcessDriver::new_retained(
        pump,
        Transport {
            outcomes: VecDeque::new(),
            writes: 0,
            closes: 0,
        },
        Control(Arc::new(Mutex::new(ControlState::default()))),
        foreign.clone(),
        Arc::new(Clock),
    ) {
        Ok(_) => panic!("foreign owner must be rejected"),
        Err(failure) => failure,
    };
    let (pump, transport, _, retained_owner, _, error) = *failure;
    assert_eq!(pump.runtime_owner(), owner());
    assert_eq!(retained_owner, foreign);
    assert_eq!(transport.closes, 0);
    assert_eq!(error, ActivationDriverError::IdentityMismatch);
}

fn intent() -> ActivationIntent {
    ActivationIntent {
        binding_digest: [7; 32],
        artifact_digest: "11".repeat(32),
        project_id: owner().project_id,
        chat_id: owner().chat_id,
        session_id: owner().session_id,
        runtime_epoch: owner().runtime_epoch,
        mode: ActivationMode::New {
            native_root: "/repo".into(),
            model: None,
        },
        curated_ceiling: capabilities(),
        sage_policy: capabilities(),
    }
}

fn identity() -> AdapterIdentity {
    AdapterIdentity {
        binding_digest: [7; 32],
        runtime_epoch: owner().runtime_epoch,
        session_id: owner().session_id,
        external_id: ExternalId::new("external", "external").unwrap(),
        turn_id: None,
        turn_client_message_id: None,
    }
}

fn capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocolVersion", "acp-test-v1").unwrap(),
        new_session: true,
        resume_session: true,
        load_session: true,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        native_session_compact: false,
        native_session_import: false,
        native_review: false,
        native_background_terminals: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: false,
        prompt_images: false,
        config_count: 0,
    }
}

fn owner() -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-activation").unwrap(),
        chat_id: SageChatId::parse("chat-activation").unwrap(),
        session_id: SageAgentSessionId::parse("session-activation").unwrap(),
        runtime_epoch: RuntimeEpoch::new(3).unwrap(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
