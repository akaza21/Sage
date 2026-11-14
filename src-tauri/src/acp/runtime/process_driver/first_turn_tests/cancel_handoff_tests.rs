use serde_json::{Value, json};

use super::model_catalog_tests::ready_adapter;
use super::*;

/// Records each native write so the test can answer the exact request ids.
struct RecordingTransport(Arc<Mutex<Vec<Value>>>);

impl crate::acp::runtime::EgressTransport for RecordingTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.0
            .lock()
            .unwrap()
            .push(serde_json::from_slice(frame.payload()).unwrap());
        FrameWriteOutcome::Complete
    }
}

impl StdinTransport for RecordingTransport {
    fn close(&mut self) {}
}

fn line(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn completed(thread: &str, turn: &str, status: &str) -> Vec<u8> {
    line(json!({"method":"turn/completed","params":{
        "threadId":thread,"turn":{"id":turn,"status":status}
    }}))
}

fn interrupted(thread: &str, turn: &str) -> Vec<u8> {
    completed(thread, turn, "interrupted")
}

struct Handoff {
    reply: Result<(), HostError>,
    reusable: bool,
}

/// Drives a real Codex turn through cancel, the interrupt ack and the
/// interrupted terminal, then queues `queued` stdout behind the host's
/// AcceptTerminal callback before the worker dequeues it. Control outranks
/// queued stdout, which is the interleaving that load produced.
async fn accept_terminal_behind(queued: fn(&str, &str) -> Vec<Vec<u8>>) -> Handoff {
    accept_native_terminal_behind(interrupted, queued).await
}

/// As `accept_terminal_behind`, with `terminal` as the native terminal frame
/// that answers the interrupt.
async fn accept_native_terminal_behind(
    terminal: fn(&str, &str) -> Vec<u8>,
    queued: fn(&str, &str) -> Vec<Vec<u8>>,
) -> Handoff {
    accept_terminal_behind_late_starts(terminal, 0, queued).await
}

/// As `accept_native_terminal_behind`, but Codex withholds its `turn/start`
/// response until after the terminal, and `late_starts` copies of it are
/// queued ahead of `queued`.
async fn accept_terminal_behind_late_starts(
    terminal: fn(&str, &str) -> Vec<u8>,
    late_starts: usize,
    queued: fn(&str, &str) -> Vec<Vec<u8>>,
) -> Handoff {
    {
        let fixture = prepare_first_turn_fixture().await;
        let adapter = ready_adapter(
            crate::acp::bindings::runtime::NativeRuntime::new(CodexRuntime::fixture_with_identity(
                Platform::WindowsX86_64,
                [0x11; 32],
                fixture.started.session_id.clone(),
            )),
            &fixture.external_session,
            true,
        );
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                adapter,
                fixture.activation_journal,
                Mapper,
                ActivatedSession {
                    binding_digest: [0x11; 32],
                    project_id: fixture.project_id.clone(),
                    chat_id: fixture.chat_id.clone(),
                    session_id: fixture.started.session_id.clone(),
                    external_session_id: fixture.external_session.clone(),
                    runtime_epoch: fixture.epoch,
                    capabilities: fixture.capabilities,
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
            transport: RecordingTransport(Arc::clone(&writes)),
            control: NoChildProcessControl {
                zero_checks: Arc::new(AtomicUsize::new(0)),
            },
            owner: fixture.owner.clone(),
            clock: Arc::new(SystemDriverClock),
        };
        let context = FirstTurnWorkerContext {
            owner: fixture.owner.clone(),
            coordinator: SendCoordinator::new(
                Arc::new(Authority(IntentSnapshot {
                    project_id: fixture.project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: fixture.chat_id.clone(),
                    projection_revision: fixture.ready_projection_revision,
                    intent_revision: fixture.ready_projection_revision,
                    session_id: Some(fixture.started.session_id.clone()),
                    runtime_epoch: Some(fixture.epoch),
                    turn_id: None,
                    turn_state: None,
                    turn_revision: None,
                })),
                fixture.journal.clone(),
                fixture.publisher.clone(),
            ),
            journal: fixture.journal.clone(),
            publisher: fixture.publisher,
            clock: Arc::new(SystemDriverClock),
            mailbox: Arc::clone(&mailbox),
            session_recovery: None,
            zero_sequence: std::sync::atomic::AtomicU64::new(0),
        };
        let mut state = FirstTurnWorkerState::Idle(Box::new(completed));

        macro_rules! step {
            ($command:expr) => {{
                let response = mailbox.submit_worker($command).unwrap();
                let ControlMessage::Worker(envelope) = mailbox
                    .wait_next_until(None, context.clock.as_ref())
                    .unwrap()
                    .expect("worker command")
                else {
                    panic!("worker envelope")
                };
                state = state.handle(&context, envelope).await;
                response.recv().unwrap()
            }};
        }

        macro_rules! run_driver_until {
            ($done:expr) => {{
                let FirstTurnWorkerState::Active(mut active) = state else {
                    panic!("active turn")
                };
                while !$done(active.turn.driver.latest_durable_turn_cursor()) {
                    active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
                }
                let cursor = active
                    .turn
                    .driver
                    .latest_durable_turn_cursor()
                    .cloned()
                    .unwrap();
                state = FirstTurnWorkerState::Active(active);
                cursor
            }};
        }

        let WorkerReply::Prepared(turn_id) =
            step!(WorkerCommand::PreparePrompt(HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id: fixture.project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: fixture.chat_id.clone(),
                    session_id: fixture.started.session_id.clone(),
                    runtime_epoch: fixture.epoch,
                    expected_projection_revision: fixture.ready_projection_revision,
                    client_intent_id: ClientIntentId::parse("intent-cancel-handoff").unwrap(),
                    prompt: Prompt::new("prompt", "cancel me").unwrap(),
                    now_millis: fixture.now_millis + 3,
                },
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            }))
            .unwrap()
        else {
            panic!("prepared turn")
        };
        step!(WorkerCommand::DispatchPrompt {
            owner: fixture.owner.clone(),
            turn_id: turn_id.clone(),
        })
        .unwrap();
        step!(WorkerCommand::ObserveSend {
            owner: fixture.owner.clone(),
            turn_id: turn_id.clone(),
            observation: crate::acp::store::SendObservation::CompleteBytes,
        })
        .unwrap();
        let prompt = writes.lock().unwrap().last().unwrap().clone();
        assert_eq!(prompt["method"], "turn/start");
        let external_turn = "fixture-turn-12345-1";
        let thread = fixture.external_session.as_str();
        let start_response =
            line(json!({"id":prompt["id"],"result":{"turn":{"id":external_turn}}}));
        if late_starts == 0 {
            mailbox.try_push_chunk(start_response.clone()).unwrap();
        }
        mailbox
            .try_push_chunk(line(json!({"method":"turn/started","params":{
                "threadId":thread,"turn":{"id":external_turn,"status":"inProgress"}
            }})))
            .unwrap();
        run_driver_until!(
            |cursor: Option<&crate::acp::store::AdapterTurnCursor>| cursor
                .is_some_and(|cursor| cursor.state() == TurnState::Running)
        );

        let cancel = HostCancelIntent {
            owner: fixture.owner.clone(),
            turn_id: turn_id.clone(),
            client_intent_id: ClientIntentId::parse("intent-cancel-handoff-cancel").unwrap(),
            now_millis: fixture.now_millis + 4,
        };
        step!(WorkerCommand::BeginCancel(cancel.clone())).unwrap();
        step!(WorkerCommand::DispatchCancel(cancel)).unwrap();
        let interrupt = writes.lock().unwrap().last().unwrap().clone();
        assert_eq!(interrupt["method"], "turn/interrupt");

        mailbox
            .try_push_chunk(line(json!({"id":interrupt["id"],"result":{}})))
            .unwrap();
        mailbox
            .try_push_chunk(terminal(thread, external_turn))
            .unwrap();
        let terminal = run_driver_until!(|cursor: Option<
            &crate::acp::store::AdapterTurnCursor,
        >| cursor
            .is_some_and(|cursor| cursor.state().is_terminal()));
        assert_eq!(terminal.turn_id(), &turn_id);
        assert!(matches!(
            terminal.state(),
            TurnState::Cancelled | TurnState::Interrupted | TurnState::Failed
        ));
        assert_eq!(mailbox.retained_usage(), ((0, 0), (0, 0)));

        let queued = std::iter::repeat_n(start_response, late_starts)
            .chain(queued(thread, external_turn))
            .collect::<Vec<_>>();
        for chunk in &queued {
            mailbox.try_push_chunk(chunk.clone()).unwrap();
        }
        let written = writes.lock().unwrap().len();
        let reply = step!(WorkerCommand::AcceptTerminal {
            owner: fixture.owner.clone(),
            cursor: terminal.clone(),
        })
        .map(|_| ());
        let queued_bytes = queued.iter().map(Vec::len).sum::<usize>();
        assert_eq!(
            mailbox.retained_usage(),
            ((queued.len(), queued_bytes), (0, 0)),
            "the handoff decision never consumes queued stdout"
        );
        let reusable = match state {
            FirstTurnWorkerState::Reusable(mut reusable) => {
                assert_eq!(reusable.terminal_cursor(), &terminal);
                // The reusable session reads the queued stdout as late frames,
                // in order and without any native write.
                for _ in 0..queued.len() {
                    let ControlMessage::StdoutChunk(chunk) = mailbox
                        .wait_next_until(None, context.clock.as_ref())
                        .unwrap()
                        .expect("queued stdout")
                    else {
                        panic!("stdout chunk")
                    };
                    reusable
                        .completed
                        .accept_idle_stdout_chunk(
                            &chunk,
                            fixture.now_millis + 5,
                            &mut reusable.transport,
                        )
                        .await
                        .unwrap();
                }
                assert!(reusable.completed.idle_frame_aligned());
                assert_eq!(writes.lock().unwrap().len(), written);
                true
            }
            FirstTurnWorkerState::Recovery(_) => false,
            _ => panic!("the terminal callback left an unexpected worker state"),
        };
        let durable_state: String = fixture
            .database
            .connection()
            .query_row(
                "SELECT state FROM turns WHERE id=?1",
                [turn_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(matches!(
            durable_state.as_str(),
            "cancelled" | "interrupted" | "failed"
        ));
        let epoch_state: String = fixture
            .database
            .connection()
            .query_row(
                "SELECT state FROM session_runtime_epochs WHERE session_id=?1 AND runtime_epoch=?2",
                rusqlite::params![
                    fixture.started.session_id.as_str(),
                    fixture.epoch.get() as i64
                ],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(epoch_state, "running");

        drop(context);
        drop(fixture.sessions);
        drop(fixture.scheduler);
        drop(fixture.journal);
        fixture.journal_task.finish();
        Handoff { reply, reusable }
    }
}

fn assert_not_reusable(handoff: Handoff) {
    assert_eq!(handoff.reply, Err(HostError::RecoveryRequired));
    assert!(!handoff.reusable);
}

/// Codex answers `turn/interrupt` with its ack and the interrupted
/// `turn/completed`, and the journey fixture then repeats that interrupted
/// terminal. When the host's terminal callback is dequeued while the repeat is
/// still queued, the worker must hand the session to its reusable state, which
/// reads the repeat as a duplicate. Before the fix the handoff demanded an
/// empty mailbox, answered RecoveryRequired, and the runner failed the
/// runtime, so a later cancel park found the epoch no longer running.
#[test]
fn cancelled_turn_hands_off_while_post_terminal_stdout_is_queued() {
    sage_test_support::test_wait::guarded("duplicate terminal behind the callback", async {
        let handoff = accept_terminal_behind(|thread, turn| {
            vec![interrupted(thread, turn), interrupted(thread, turn)]
        })
        .await;
        assert_eq!(handoff.reply, Ok(()));
        assert!(handoff.reusable);
    });
}

#[test]
fn queued_terminal_for_another_turn_keeps_the_handoff_closed() {
    sage_test_support::test_wait::guarded("foreign terminal behind the callback", async {
        assert_not_reusable(
            accept_terminal_behind(|thread, _| vec![interrupted(thread, "fixture-turn-other")])
                .await,
        );
    });
}

#[test]
fn queued_terminal_of_another_kind_keeps_the_handoff_closed() {
    sage_test_support::test_wait::guarded("other terminal kind behind the callback", async {
        assert_not_reusable(
            accept_terminal_behind(|thread, turn| vec![completed(thread, turn, "completed")]).await,
        );
    });
}

#[test]
fn unrelated_queued_stdout_keeps_the_handoff_closed() {
    sage_test_support::test_wait::guarded("unrelated stdout behind the callback", async {
        assert_not_reusable(
            accept_terminal_behind(|thread, turn| {
                vec![
                    interrupted(thread, turn),
                    line(json!({"method":"thread/tokenUsage/updated","params":{
                        "threadId":thread,"turnId":turn,"tokenUsage":{}
                    }})),
                ]
            })
            .await,
        );
    });
}

#[test]
fn split_duplicate_terminal_keeps_the_handoff_closed() {
    sage_test_support::test_wait::guarded("split terminal behind the callback", async {
        assert_not_reusable(
            accept_terminal_behind(|thread, turn| {
                let mut duplicate = interrupted(thread, turn);
                duplicate.truncate(duplicate.len() / 2);
                vec![duplicate]
            })
            .await,
        );
    });
}

/// Same thread, turn and status, but a repeated key that ingestion's
/// structural validation rejects.
#[test]
fn duplicate_key_terminal_keeps_the_handoff_closed() {
    sage_test_support::test_wait::guarded("duplicate-key terminal behind the callback", async {
        assert_not_reusable(
            accept_terminal_behind(|thread, turn| {
                vec![
                    format!(
                        "{}\n",
                        r#"{"method":"turn/completed","method":"turn/completed","params":{"threadId":"THREAD","turn":{"id":"TURN","status":"interrupted"}}}"#
                            .replace("THREAD", thread)
                            .replace("TURN", turn)
                    )
                    .into_bytes(),
                ]
            })
            .await,
        );
    });
}

/// Same thread, turn and status, but a top-level `result` makes ingestion
/// route it as a response without an id and reject it.
#[test]
fn terminal_with_a_result_member_keeps_the_handoff_closed() {
    sage_test_support::test_wait::guarded("result-bearing terminal behind the callback", async {
        assert_not_reusable(
            accept_terminal_behind(|thread, turn| {
                vec![line(
                    json!({"method":"turn/completed","result":{},"params":{
                        "threadId":thread,"turn":{"id":turn,"status":"interrupted"}
                    }}),
                )]
            })
            .await,
        );
    });
}

fn failed(thread: &str, turn: &str) -> Vec<u8> {
    line(
        json!({"method":"turn/completed","params":{"threadId":thread,"turn":{
            "id":turn,"status":"failed","error":{"message":"native failure"}
        }}}),
    )
}

/// The failed counterpart of the positive case: a valid repeat of a failed
/// terminal is the duplicate idle ingestion ignores.
#[test]
fn failed_turn_hands_off_while_its_duplicate_terminal_is_queued() {
    sage_test_support::test_wait::guarded("duplicate failed terminal behind the callback", async {
        let handoff =
            accept_native_terminal_behind(failed, |thread, turn| vec![failed(thread, turn)]).await;
        assert_eq!(handoff.reply, Ok(()));
        assert!(handoff.reusable);
    });
}

/// Same thread, turn and failed status, but an `error` that is not an
/// object: idle ingestion rejects its payload before it reads the terminal
/// as a duplicate, so the proof must too.
#[test]
fn failed_terminal_with_a_malformed_error_keeps_the_handoff_closed() {
    sage_test_support::test_wait::guarded("malformed failed terminal behind the callback", async {
        assert_not_reusable(
            accept_native_terminal_behind(failed, |thread, turn| {
                vec![line(
                    json!({"method":"turn/completed","params":{"threadId":thread,"turn":{
                        "id":turn,"status":"failed","error":42
                    }}}),
                )]
            })
            .await,
        );
    });
}

/// Codex answers `turn/start` only after the interrupted terminal, and the
/// late response is queued twice. Each copy reads as a duplicate terminal on
/// its own, but the first settles the pending `turn/start` RPC, so idle
/// ingestion would reject the second as uncorrelated. The proof reads them in
/// order on one copy of the adapter and sees the state change.
#[test]
fn queued_late_turn_start_responses_keep_the_handoff_closed() {
    sage_test_support::test_wait::guarded("late turn/start responses behind the callback", async {
        assert_not_reusable(
            accept_terminal_behind_late_starts(interrupted, 2, |_, _| Vec::new()).await,
        );
    });
}
