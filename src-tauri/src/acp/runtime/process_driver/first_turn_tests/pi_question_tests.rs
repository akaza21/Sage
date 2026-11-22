use super::*;
use crate::acp::bindings::pi::PiRuntime;
use sage_acp_protocol::access_mode::SessionAccessMode;
use serde_json::{Value, json};

struct Wire(Arc<Mutex<Vec<Value>>>);

impl crate::acp::runtime::EgressTransport for Wire {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.0
            .lock()
            .unwrap()
            .push(serde_json::from_slice(frame.payload()).unwrap());
        FrameWriteOutcome::Complete
    }
}

impl StdinTransport for Wire {
    fn close(&mut self) {}
}

fn line(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn respond(adapter: &mut PiRuntime, egress: AdapterEgress, data: Value) -> AdapterBatch {
    let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    adapter
        .resolve_egress(egress.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    let frame = crate::acp::transport::IncrementalFrameDecoder::new().push_chunk(&line(json!({
        "type":"response", "id":request["id"], "command":request["type"], "success":true, "data":data
    }))).unwrap().unwrap();
    adapter.ingest(&frame, &Mapper).unwrap()
}

#[test]
fn pi_questions_between_prepare_and_dispatch_are_declined_before_the_prompt() {
    sage_test_support::test_wait::guarded("Pi question during first prompt preparation", async {
        let fixture = prepare_first_turn_fixture().await;
        let mut adapter = PiRuntime::from_native_binding(
            [0x11; 32],
            fixture.epoch,
            fixture.started.session_id.clone(),
        );
        let init = adapter.command(AdapterCommand::Initialize).unwrap();
        let models = respond(
            &mut adapter,
            init,
            json!({"model":{"provider":"openai","id":"gpt","name":"GPT"}}),
        )
        .into_parts()
        .1
        .remove(0);
        respond(
            &mut adapter,
            models,
            json!({"models":[{"provider":"openai","id":"gpt","name":"GPT"}]}),
        );
        let open = adapter
            .command(AdapterCommand::NewSession {
                native_root: fixture.root.path().to_str().unwrap().into(),
                model: None,
            })
            .unwrap();
        respond(
            &mut adapter,
            open,
            json!({"sessionId":fixture.external_session.as_str()}),
        );
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let mut state = FirstTurnWorkerState::Idle(Box::new(CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                adapter,
                fixture.activation_journal,
                Mapper,
                ActivatedSession {
                    binding_digest: [0x11; 32],
                    project_id: fixture.project_id.clone(),
                    chat_id: fixture.chat_id.clone(),
                    session_id: fixture.started.session_id.clone(),
                    external_session_id: fixture.external_session,
                    runtime_epoch: fixture.epoch,
                    capabilities: fixture.capabilities,
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            idle_decoder: crate::acp::transport::IncrementalFrameDecoder::new(),
            transport: Wire(Arc::clone(&writes)),
            control: NoChildProcessControl {
                zero_checks: Default::default(),
                harness_error: None,
            },
            owner: fixture.owner.clone(),
            clock: Arc::new(SystemDriverClock),
        }));
        let authority = Arc::new(Authority(IntentSnapshot {
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
        }));
        let context = FirstTurnWorkerContext {
            owner: fixture.owner.clone(),
            coordinator: SendCoordinator::new(
                authority,
                fixture.journal.clone(),
                fixture.publisher.clone(),
            ),
            journal: fixture.journal.clone(),
            publisher: fixture.publisher,
            clock: Arc::new(SystemDriverClock),
            mailbox: Arc::clone(&mailbox),
            session_recovery: None,
            zero_sequence: Default::default(),
        };
        macro_rules! step {
            ($command:expr) => {{
                let response = mailbox.submit_worker($command).unwrap();
                let Some(ControlMessage::Worker(envelope)) = mailbox
                    .wait_next_until(None, context.clock.as_ref())
                    .unwrap()
                else {
                    panic!("worker command")
                };
                state = state.handle(&context, envelope).await;
                response.recv().unwrap()
            }};
        }
        let Ok(WorkerReply::Prepared(turn_id)) =
            step!(WorkerCommand::PreparePrompt(HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id: fixture.project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: fixture.chat_id.clone(),
                    session_id: fixture.started.session_id.clone(),
                    runtime_epoch: fixture.epoch,
                    expected_projection_revision: fixture.ready_projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    prompt: Prompt::new("prompt", "hello").unwrap(),
                    now_millis: fixture.now_millis + 3,
                },
                access_mode: SessionAccessMode::ReadOnly,
            }))
        else {
            panic!("prepared prompt")
        };
        let FirstTurnWorkerState::Prepared { turn, .. } = &mut state else {
            panic!("prepared state")
        };
        let mut questions = line(
            json!({"type":"extension_ui_request","id":"before-dispatch","method":"input","title":"Name?"}),
        );
        // The second frame stays in the idle decoder after the first starts adoption.
        questions.extend(line(
            json!({"type":"extension_ui_request","id":"buffered","method":"input","title":"Next?"}),
        ));
        turn.parts.accept_idle_stdout_chunk(&questions).unwrap();
        assert!(matches!(
            step!(WorkerCommand::DispatchPrompt {
                owner: fixture.owner.clone(),
                turn_id: turn_id.clone()
            }),
            Ok(WorkerReply::Write(_))
        ));
        {
            let writes = writes.lock().unwrap();
            assert_eq!(
                writes[0],
                json!({"type":"extension_ui_response","id":"before-dispatch","cancelled":true})
            );
            assert_eq!(
                writes[1],
                json!({"type":"extension_ui_response","id":"buffered","cancelled":true})
            );
            assert_eq!(writes[2]["type"], "prompt");
        }
        assert!(matches!(
            step!(WorkerCommand::ObserveSend {
                owner: fixture.owner.clone(),
                turn_id,
                observation: crate::acp::store::SendObservation::CompleteBytes,
            }),
            Ok(WorkerReply::Unit)
        ));
        let FirstTurnWorkerState::Active(active) = &mut state else {
            panic!("active prompt")
        };
        mailbox.try_push_chunk(line(json!({"type":"extension_ui_request","id":"after-dispatch","method":"input","title":"Answer?"}))).unwrap();
        active.turn.driver.run_next(&mailbox).await.unwrap();
        let page = crate::acp::store::load_chat_page(
            &fixture.database.connection(),
            fixture.project_id,
            fixture.chat_id,
            None,
        )
        .unwrap();
        assert!(
            page.turns
                .as_slice()
                .iter()
                .flat_map(|turn| turn.items.as_slice())
                .any(|item| matches!(
                    &item.content,
                    crate::acp::projection::ItemContent::PermissionRequest {
                        user_input: Some(_),
                        ..
                    }
                )),
            "a later question opens a card instead of queuing behind abandoned adoption"
        );
        drop(state);
        drop(context);
        drop(fixture.sessions);
        drop(fixture.scheduler);
        drop(fixture.journal);
        fixture.journal_task.finish();
    });
}
