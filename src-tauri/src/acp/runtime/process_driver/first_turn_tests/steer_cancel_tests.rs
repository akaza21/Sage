use super::*;

#[test]
fn queued_steer_overtaken_by_cancel_never_writes() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let FirstTurnFixture {
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
            mut capabilities,
            external_session,
            owner,
            ready_projection_revision,
            root,
        } = prepare_first_turn_fixture().await;
        capabilities.active_turn_steer = true;
        let writes = Arc::new(AtomicUsize::new(0));
        let zero_checks = Arc::new(AtomicUsize::new(0));
        let completed = CompletedActivationDriverParts {
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
            control: NoChildProcessControl { zero_checks },
            owner: owner.clone(),
            clock: Arc::new(SystemDriverClock),
        };
        let authority = Arc::new(Authority(IntentSnapshot {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            projection_revision: ready_projection_revision,
            intent_revision: ready_projection_revision,
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
            clock: Arc::new(SystemDriverClock),
            mailbox: Arc::clone(&mailbox),
            session_recovery: None,
            zero_sequence: std::sync::atomic::AtomicU64::new(0),
        };
        let mut state = FirstTurnWorkerState::Idle(completed);
        let prepare = mailbox
            .submit_worker(WorkerCommand::PreparePrompt(HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id,
                    project_generation: ProjectGeneration::ZERO,
                    chat_id,
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: ready_projection_revision,
                    client_intent_id: ClientIntentId::parse("intent-steer-cancel-turn").unwrap(),
                    prompt: Prompt::new("prompt", "start").unwrap(),
                    now_millis: now_millis + 3,
                },
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            }))
            .unwrap();
        state = handle_next(state, &context).await;
        let WorkerReply::Prepared(turn_id) = prepare.recv().unwrap().unwrap() else {
            panic!("prompt was not prepared")
        };
        let dispatch = mailbox
            .submit_worker(WorkerCommand::DispatchPrompt {
                owner: owner.clone(),
                turn_id: turn_id.clone(),
            })
            .unwrap();
        state = handle_next(state, &context).await;
        assert!(matches!(
            dispatch.recv().unwrap(),
            Ok(WorkerReply::Write(LiveWrite::Complete))
        ));
        let observe = mailbox
            .submit_worker(WorkerCommand::ObserveSend {
                owner: owner.clone(),
                turn_id: turn_id.clone(),
                observation: crate::acp::store::SendObservation::CompleteBytes,
            })
            .unwrap();
        state = handle_next(state, &context).await;
        assert!(matches!(observe.recv().unwrap(), Ok(WorkerReply::Unit)));

        let (completion_send, completion_receive) = mpsc::sync_channel(2);
        let steer = mailbox
            .submit_worker(WorkerCommand::Steer {
                intent: crate::acp::runtime::HostSteerIntent {
                    owner: owner.clone(),
                    turn_id: turn_id.clone(),
                    prompt: Prompt::new("prompt", "must not steer").unwrap(),
                    client_message_id: RequiredText::new("client", "steer-after-cancel").unwrap(),
                },
                completion: crate::acp::runtime::SteerCompletionSink::new(move |result| {
                    let _ = completion_send.send(result);
                }),
            })
            .unwrap();
        let cancel = HostCancelIntent {
            owner: owner.clone(),
            turn_id,
            client_intent_id: ClientIntentId::parse("intent-steer-cancel").unwrap(),
            now_millis: now_millis + 4,
        };
        let begin_cancel = mailbox
            .submit_worker(WorkerCommand::BeginCancel(cancel.clone()))
            .unwrap();
        state = handle_next(state, &context).await;
        assert!(matches!(
            begin_cancel.recv().unwrap(),
            Ok(WorkerReply::Unit)
        ));
        let before_steer = writes.load(Ordering::SeqCst);
        state = handle_next(state, &context).await;
        assert!(matches!(steer.recv().unwrap(), Err(HostError::Intent)));
        assert_eq!(completion_receive.recv().unwrap(), Err(HostError::Intent));
        assert!(matches!(
            completion_receive.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert_eq!(writes.load(Ordering::SeqCst), before_steer);

        drop(state);
        drop(context);
        drop(sessions);
        drop(scheduler);
        drop(journal);
        journal_task.finish();
        drop(database);
        drop(root);
    });
}

async fn handle_next(
    state: FirstTurnWorkerState<Adapter, Mapper, CancelTransport, NoChildProcessControl>,
    context: &FirstTurnWorkerContext<Authority>,
) -> FirstTurnWorkerState<Adapter, Mapper, CancelTransport, NoChildProcessControl> {
    let message = context
        .mailbox
        .wait_next_until(None, context.clock.as_ref())
        .unwrap()
        .expect("worker command");
    let ControlMessage::Worker(envelope) = message else {
        panic!("expected worker command")
    };
    state.handle(context, envelope).await
}
