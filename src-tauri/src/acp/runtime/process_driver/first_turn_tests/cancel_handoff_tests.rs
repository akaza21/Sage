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

/// Codex answers `turn/interrupt` with its ack and the interrupted
/// `turn/completed`, and can emit more stdout after that terminal (the journey
/// fixture repeats the interrupted terminal). When the host's terminal
/// callback is dequeued while such stdout is still queued, the worker must
/// still hand the session to its reusable state; the idle ingress then reads
/// the queued stdout in order. Before the fix the handoff demanded an empty
/// mailbox, answered RecoveryRequired, and the runner failed the runtime, so a
/// later cancel park found the epoch no longer running.
#[test]
fn cancelled_turn_hands_off_while_post_terminal_stdout_is_queued() {
    crate::test_wait::guarded("cancelled turn handoff with queued stdout", async {
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
        let mut state = FirstTurnWorkerState::Idle(completed);

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
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
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
        mailbox
            .try_push_chunk(line(
                json!({"id":prompt["id"],"result":{"turn":{"id":external_turn}}}),
            ))
            .unwrap();
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

        let interrupted = line(json!({"method":"turn/completed","params":{
            "threadId":thread,"turn":{"id":external_turn,"status":"interrupted"}
        }}));
        mailbox
            .try_push_chunk(line(json!({"id":interrupt["id"],"result":{}})))
            .unwrap();
        mailbox.try_push_chunk(interrupted.clone()).unwrap();
        let terminal = run_driver_until!(|cursor: Option<
            &crate::acp::store::AdapterTurnCursor,
        >| cursor
            .is_some_and(|cursor| cursor.state().is_terminal()));
        assert_eq!(terminal.turn_id(), &turn_id);
        assert!(matches!(
            terminal.state(),
            TurnState::Cancelled | TurnState::Interrupted
        ));
        assert_eq!(mailbox.retained_usage(), ((0, 0), (0, 0)));

        // The native stream continues after the durable terminal, and the
        // host's terminal callback is dequeued first because control outranks
        // queued stdout. This is the interleaving that load produced.
        mailbox.try_push_chunk(interrupted.clone()).unwrap();
        let written = writes.lock().unwrap().len();
        assert_eq!(
            step!(WorkerCommand::AcceptTerminal {
                owner: fixture.owner.clone(),
                cursor: terminal.clone(),
            })
            .map(|_| ()),
            Ok(())
        );
        let FirstTurnWorkerState::Reusable(mut reusable) = state else {
            panic!("the cancelled turn must hand off to the reusable session")
        };
        assert_eq!(reusable.terminal_cursor(), &terminal);
        assert_eq!(
            mailbox.retained_usage(),
            ((1, interrupted.len()), (0, 0)),
            "the queued stdout stays in stream order for the reusable session"
        );

        // The reusable session reads the queued duplicate terminal as late
        // stdout without any native write.
        let ControlMessage::StdoutChunk(chunk) = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .expect("queued stdout")
        else {
            panic!("stdout chunk")
        };
        reusable
            .completed
            .accept_idle_stdout_chunk(&chunk, fixture.now_millis + 5, &mut reusable.transport)
            .await
            .unwrap();
        assert!(reusable.completed.idle_frame_aligned());
        assert_eq!(writes.lock().unwrap().len(), written);
        assert_eq!(mailbox.retained_usage(), ((0, 0), (0, 0)));
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
            "cancelled" | "interrupted"
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

        drop(reusable);
        drop(context);
        drop(fixture.sessions);
        drop(fixture.scheduler);
        drop(fixture.journal);
        fixture.journal_task.finish();
    });
}
