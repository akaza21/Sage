use super::*;

#[test]
fn committed_cancellation_is_an_intentional_shutdown_before_terminal_acknowledgement() {
    crate::test_wait::guarded("committed cancellation shutdown", async {
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
            capabilities,
            external_session,
            owner,
            ready_projection_revision,
            root,
        } = prepare_first_turn_fixture().await;
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                SequencedAdapter {
                    binding_digest: [0x11; 32],
                    runtime_epoch: epoch,
                    session_id: started.session_id.clone(),
                    external_session_id: external_session.clone(),
                    pending_message: None,
                    prompt_ids: Arc::new(Mutex::new(Vec::new())),
                    cancel_count: Arc::new(AtomicUsize::new(0)),
                    terminal_states: Arc::new(Mutex::new(VecDeque::from([TurnState::Cancelled]))),
                    next_request: 0,
                    next_external_turn: 0,
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
            transport: Transport(Mutex::new(VecDeque::new())),
            control: NoChildProcessControl {
                zero_checks: Arc::new(AtomicUsize::new(0)),
            },
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
        let mut state = FirstTurnWorkerState::Idle(Box::new(completed));
        macro_rules! step {
            ($command:expr) => {{
                let response = mailbox.submit_worker($command).unwrap();
                let Some(ControlMessage::Worker(envelope)) = mailbox
                    .wait_next_until(None, context.clock.as_ref())
                    .unwrap()
                else {
                    panic!("queued worker command")
                };
                state = state.handle(&context, envelope).await;
                response.recv().unwrap().unwrap()
            }};
        }
        let WorkerReply::Prepared(turn_id) =
            step!(WorkerCommand::PreparePrompt(HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id,
                    project_generation: ProjectGeneration::ZERO,
                    chat_id,
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: ready_projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    prompt: Prompt::new("prompt", "cancel this turn").unwrap(),
                    now_millis: now_millis + 3,
                },
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            }))
        else {
            panic!("prepared turn")
        };
        step!(WorkerCommand::DispatchPrompt {
            owner: owner.clone(),
            turn_id: turn_id.clone()
        });
        step!(WorkerCommand::ObserveSend {
            owner: owner.clone(),
            turn_id: turn_id.clone(),
            observation: crate::acp::store::SendObservation::CompleteBytes,
        });
        step!(WorkerCommand::BeginCancel(HostCancelIntent {
            owner,
            turn_id: turn_id.clone(),
            client_intent_id: ClientIntentId::new(),
            now_millis: now_millis + 4,
        }));
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        let FirstTurnWorkerState::Active(mut active) = state else {
            panic!("active turn")
        };
        active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
        assert_eq!(active.turn.dispatch.state(), TurnState::Cancelling);
        let committed: String = database
            .connection()
            .query_row(
                "SELECT state FROM turns WHERE id=?1",
                [turn_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(committed, "cancelled");

        // The service can observe that commit and enqueue Shutdown before the
        // host's terminal callback has acknowledged it. Preserve that ordering.
        let _response = mailbox.submit_worker(WorkerCommand::Shutdown).unwrap();
        let Some(ControlMessage::Worker(envelope)) = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
        else {
            panic!("queued shutdown")
        };
        let exit = super::super::live_runner::requested_shutdown(
            FirstTurnWorkerState::Active(active),
            envelope,
        );
        assert_eq!(
            exit.reason,
            TreeExit::Cancelled,
            "a durably cancelled turn must not trigger failed-epoch recovery"
        );
        drop(exit);
        drop(context);
        drop(sessions);
        drop(scheduler);
        drop(journal);
        journal_task.finish();
        drop(database);
        drop(root);
    });
}
