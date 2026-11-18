use super::*;
use crate::acp::{
    bindings::adapter::RefusalDetail,
    projection::TurnTerminalReasonProjection,
    runtime::{JournalLane, terminal_detail},
    store::RecoverRuntimeCommand,
};
use sage_acp_protocol::lifecycle::TurnTerminalReason;

const NOTICE: &str = "Claude Code is signed out";
const REMEDIATION: &str = "Run claude auth login, then send again";

/// A harness that refuses every prompt for sign-in before Sage writes it.
struct SignedOutAdapter {
    next_request: u64,
}

impl HarnessAdapter for SignedOutAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if matches!(command, AdapterCommand::Prompt { .. }) {
            return Err(AdapterRuntimeError::SignInRequired);
        }
        self.next_request += 1;
        Ok(AdapterEgress::new(
            OutboundFrame::new(b"{}".to_vec()).unwrap(),
            EgressToken::request(self.next_request),
        ))
    }

    fn refusal_detail(&self) -> Option<RefusalDetail> {
        RefusalDetail::new(Some(NOTICE), Some(REMEDIATION))
    }

    fn ingest(
        &mut self,
        _: &Frame,
        _: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        Err(AdapterRuntimeError::Operation)
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

    fn unfinished_replies(&mut self) -> Result<Vec<AdapterEvent>, AdapterRuntimeError> {
        Ok(Vec::new())
    }

    fn held_batch(&mut self) -> Result<Option<AdapterBatch>, AdapterRuntimeError> {
        Ok(None)
    }
}

fn terminal_reason(
    database: &Database,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    turn_id: &sage_acp_protocol::contract::SageTurnId,
) -> (TurnState, Option<TurnTerminalReasonProjection>) {
    let page = crate::acp::store::load_chat_page(
        &database.connection(),
        project_id.clone(),
        chat_id.clone(),
        None,
    )
    .unwrap();
    let turn = page
        .turns
        .as_slice()
        .iter()
        .find(|turn| &turn.id == turn_id)
        .expect("refused turn");
    (turn.state, turn.terminal_reason.clone())
}

#[test]
fn a_prompt_refused_for_sign_in_before_its_write_fails_as_auth_required_through_release() {
    sage_test_support::test_wait::guarded("sign-in refusal before write", async {
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
                SignedOutAdapter { next_request: 0 },
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
                harness_error: None,
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
                response.recv().unwrap()
            }};
        }
        let Ok(WorkerReply::Prepared(turn_id)) =
            step!(WorkerCommand::PreparePrompt(HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id: project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: ready_projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    prompt: Prompt::new("prompt", "hello").unwrap(),
                    now_millis: now_millis + 3,
                },
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            }))
        else {
            panic!("prepared turn")
        };
        assert!(matches!(
            step!(WorkerCommand::DispatchPrompt {
                owner: owner.clone(),
                turn_id: turn_id.clone()
            }),
            Err(HostError::SignInRequired)
        ));
        let auth_required = Some(TurnTerminalReasonProjection {
            kind: TurnTerminalReason::AuthRequired,
            message: Some(NOTICE.to_owned()),
            remediation: Some(REMEDIATION.to_owned()),
        });
        assert_eq!(
            terminal_reason(&database, &project_id, &chat_id, &turn_id),
            (TurnState::Failed, auth_required.clone()),
            "the refusal settles the turn before Sage releases the runtime"
        );

        // The service then releases the runtime; the runner records whatever
        // the release reports for a turn that is still open.
        let _response = mailbox.submit_worker(WorkerCommand::Shutdown).unwrap();
        let Some(ControlMessage::Worker(envelope)) = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
        else {
            panic!("queued shutdown")
        };
        let teardown = state.teardown_outcome();
        let exit = super::super::live_runner::requested_shutdown(state, envelope);
        let release = terminal_detail::runtime_lost(exit.reason, teardown, None);
        journal
            .enqueue_one(
                JournalLane::Control,
                StoreMutation::RecoverRuntimeAfterZero(RecoverRuntimeCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    terminal_reason: release,
                    now_millis: now_millis + 4,
                }),
                u64::try_from(sage_core::time::now_millis()).unwrap() + 5_000,
            )
            .unwrap()
            .committed()
            .await
            .unwrap();
        assert_eq!(
            terminal_reason(&database, &project_id, &chat_id, &turn_id),
            (TurnState::Failed, auth_required),
            "releasing the runtime never rewrites the refused turn's reason"
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
