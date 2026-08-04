use std::{
    collections::VecDeque,
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use uuid::Uuid;

use crate::{
    acp::{
        bindings::adapter::{
            AdapterBatch, AdapterCommand, AdapterEgress, AdapterEvent, AdapterIdentity,
            AdapterRuntimeError, EgressResolution, EgressToken, HarnessAdapter, ResourceMapper,
        },
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RequiredText, RuntimeEpoch,
            SageChatId, SageProjectId, SchemaVersionV1,
        },
        intent::{CapabilitySnapshotV1, ExternalId, IntentSnapshot, Prompt, SafeResourceRef},
        lifecycle::{DeliveryState, TurnState},
        process::{LaunchError, TreeExit},
        runtime::{
            ActivatedSession, ActivationJournal, CoordinatorActivationJournal, HostCancelIntent,
            HostError, HostPromptIntent, IdleActivationParts, JournalHandle, LiveRuntimePort,
            LiveWrite, ProjectIntentAuthority, ProjectionMailbox, SendCoordinator,
            SessionCoordinator,
        },
        scheduler::{RuntimeOwner, Scheduler},
        store::{
            AcpStore, ActivateSessionCommand, CompleteActivationCommand, DatabaseAcpStore,
            PrepareTurnCommand, StartSessionCommand, StoreCommit, StoreError, StoreMutation,
        },
        transport::{Frame, FrameWriteOutcome, OutboundFrame},
    },
    persistence::Database,
};

use super::*;

struct Authority(IntentSnapshot);

struct MutableAuthority(Arc<Mutex<IntentSnapshot>>);

struct Adapter {
    cancel_count: Arc<AtomicUsize>,
}

struct SequencedAdapter {
    binding_digest: [u8; 32],
    runtime_epoch: RuntimeEpoch,
    session_id: crate::acp::contract::SageAgentSessionId,
    external_session_id: ExternalId,
    pending_message: Option<RequiredText<512>>,
    prompt_ids: Arc<Mutex<Vec<String>>>,
    next_request: u64,
    next_external_turn: u64,
}

impl HarnessAdapter for Adapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        let token = if matches!(command, AdapterCommand::Cancel) {
            self.cancel_count.fetch_add(1, Ordering::SeqCst);
            EgressToken::cancel_request(1)
        } else {
            EgressToken::request(1)
        };
        Ok(AdapterEgress::new(
            OutboundFrame::new(b"{}".to_vec()).unwrap(),
            token,
        ))
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
}

impl HarnessAdapter for SequencedAdapter {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        None
    }

    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        if let AdapterCommand::Prompt {
            client_message_id, ..
        } = command
        {
            self.prompt_ids
                .lock()
                .unwrap()
                .push(client_message_id.as_str().to_owned());
            self.pending_message = Some(client_message_id);
        }
        self.next_request += 1;
        Ok(AdapterEgress::new(
            OutboundFrame::new(b"{}".to_vec()).unwrap(),
            EgressToken::request(self.next_request),
        ))
    }

    fn ingest(
        &mut self,
        _: &Frame,
        _: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let message = self
            .pending_message
            .take()
            .ok_or(AdapterRuntimeError::Operation)?;
        self.next_external_turn += 1;
        let identity = AdapterIdentity {
            binding_digest: self.binding_digest,
            runtime_epoch: self.runtime_epoch,
            session_id: self.session_id.clone(),
            external_id: self.external_session_id.clone(),
            turn_id: Some(
                ExternalId::new(
                    "externalTurn",
                    format!("external-turn-{}", self.next_external_turn),
                )
                .unwrap(),
            ),
            turn_client_message_id: Some(message),
        };
        AdapterBatch::new(
            vec![
                AdapterEvent::Turn {
                    id: identity.clone(),
                    state: TurnState::Running,
                },
                AdapterEvent::Turn {
                    id: identity,
                    state: TurnState::Completed,
                },
            ],
            Vec::new(),
        )
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

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _: &str,
    ) -> Result<SafeResourceRef, crate::acp::intent::UnavailableResourceReason> {
        Err(crate::acp::intent::UnavailableResourceReason::Unsupported)
    }
}

struct Transport(Mutex<VecDeque<FrameWriteOutcome>>);

impl crate::acp::runtime::EgressTransport for Transport {
    fn write(&mut self, _: &OutboundFrame) -> FrameWriteOutcome {
        self.0
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(FrameWriteOutcome::Complete)
    }
}

impl StdinTransport for Transport {
    fn close(&mut self) {}
}

struct Control {
    zero_waits: Arc<AtomicUsize>,
}

struct CommitUnknownOnceStore {
    inner: DatabaseAcpStore,
    begin_send: Arc<Mutex<Option<bool>>>,
}

impl AcpStore for CommitUnknownOnceStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let injection = if matches!(batch, [StoreMutation::BeginSend(_)]) {
            self.begin_send.lock().unwrap().take()
        } else {
            None
        };
        match injection {
            Some(true) => {
                self.inner.commit(batch)?;
                Err(StoreError::CommitUnknown)
            }
            Some(false) => Err(StoreError::CommitUnknown),
            None => self.inner.commit(batch),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CloseBeforeWriteCase {
    Idle,
    Prepared,
    BeginSendAppliedUnknown,
    BeginSendNotAppliedUnknown,
}

impl CloseBeforeWriteCase {
    fn prepares_turn(self) -> bool {
        self != Self::Idle
    }

    fn begin_send_injection(self) -> Option<bool> {
        match self {
            Self::BeginSendAppliedUnknown => Some(true),
            Self::BeginSendNotAppliedUnknown => Some(false),
            Self::Idle | Self::Prepared => None,
        }
    }
}

impl ProcessTreeControl for Control {
    type ZeroProof = ();

    fn request_termination(&self, _: TreeExit) -> Result<(), LaunchError> {
        Ok(())
    }

    fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        Err(LaunchError::PlatformUnavailable)
    }

    fn wait_zero(&self) -> Result<(TreeExit, Self::ZeroProof), LaunchError> {
        self.zero_waits.fetch_add(1, Ordering::SeqCst);
        Ok((TreeExit::Cancelled, ()))
    }
}

impl ProjectIntentAuthority for Authority {
    fn current_snapshot(&self, _: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.clone())
    }
}

impl ProjectIntentAuthority for MutableAuthority {
    fn current_snapshot(&self, _: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.lock().unwrap().clone())
    }
}

#[test]
fn first_turn_retry_cancel_and_close_recovery_keep_one_exact_identity() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let root = std::env::temp_dir().join(format!("sage-first-turn-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse("chat-first-turn").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES (?1,?2,1,'First Turn',0,?3,?3)",
                rusqlite::params![chat_id.as_str(), project_id.as_str(), now],
            )
            .unwrap();
        let publisher = Arc::new(ProjectionMailbox::default());
        let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let scheduler = Scheduler::new();
        let sessions =
            SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
        let initial = crate::acp::store::load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let binding = "11".repeat(32);
        let artifact = "22".repeat(32);
        let started = sessions
            .start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: initial.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-start").unwrap(),
                binding_id: RequiredText::new("binding", "binding-test").unwrap(),
                binding_digest: binding.clone(),
                harness_version: None,
                model_id: None,
                now_millis: now,
            })
            .await
            .unwrap();
        let epoch = RuntimeEpoch::new(1).unwrap();
        let lease = sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: artifact.clone(),
                now_millis: now + 1,
            })
            .await
            .unwrap();
        let capabilities = capabilities();
        let external_session = ExternalId::new("external", "external-session").unwrap();
        let mut activation_journal = CoordinatorActivationJournal::new(lease);
        let ready = activation_journal
            .commit_ready(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: binding,
                artifact_digest: artifact,
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities).unwrap(),
                now_millis: now + 2,
            })
            .await
            .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
        };
        let clock: Arc<dyn DriverClock> = Arc::new(SystemDriverClock);
        let zero_waits = Arc::new(AtomicUsize::new(0));
        let cancel_count = Arc::new(AtomicUsize::new(0));
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                Adapter {
                    cancel_count: Arc::clone(&cancel_count),
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
            transport: Transport(Mutex::new(VecDeque::from([
                FrameWriteOutcome::ZeroByteFailure,
                FrameWriteOutcome::Complete,
                FrameWriteOutcome::DeliveryUnknown { bytes_written: 1 },
            ]))),
            control: Control {
                zero_waits: Arc::clone(&zero_waits),
            },
            owner: owner.clone(),
            clock: Arc::clone(&clock),
        };
        let authority = Arc::new(Authority(IntentSnapshot {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            projection_revision: ready.projection_revision,
            session_id: Some(started.session_id.clone()),
            runtime_epoch: Some(epoch),
            turn_id: None,
            turn_state: None,
            turn_revision: None,
        }));
        let sends = SendCoordinator::new(authority, journal.clone(), publisher.clone());
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let context = FirstTurnWorkerContext {
            owner: owner.clone(),
            coordinator: sends,
            journal: journal.clone(),
            publisher: publisher.clone(),
            clock,
            mailbox: Arc::clone(&mailbox),
            zero_sequence: std::sync::atomic::AtomicU64::new(0),
        };
        let caller_mailbox = Arc::clone(&mailbox);
        let caller_owner = owner.clone();
        let caller = std::thread::spawn(move || {
            let mut port = ProcessLivePort::new(
                caller_owner.clone(),
                caller_mailbox,
                Arc::new(|_| {}),
            );
            let turn_id = port
                .prepare_prompt(&HostPromptIntent {
                    command: PrepareTurnCommand {
                        project_id: caller_owner.project_id.clone(),
                        project_generation: ProjectGeneration::ZERO,
                        chat_id: caller_owner.chat_id.clone(),
                        session_id: caller_owner.session_id.clone(),
                        runtime_epoch: caller_owner.runtime_epoch,
                        expected_projection_revision: ready.projection_revision,
                        client_intent_id: ClientIntentId::parse("intent-turn").unwrap(),
                        prompt: Prompt::new("prompt", "hello").unwrap(),
                        now_millis: now + 3,
                    },
                })
                .unwrap();
            assert_eq!(
                port.dispatch_prompt(&caller_owner, &turn_id),
                Err(HostError::RecoveryRequired)
            );
            let first = port.dispatch_prompt(&caller_owner, &turn_id).unwrap();
            port.observe_send(
                &caller_owner,
                &turn_id,
                crate::acp::store::SendObservation::ZeroBytes,
            )
            .unwrap();
            let retry = port.dispatch_prompt(&caller_owner, &turn_id).unwrap();
            assert_eq!(
                port.observe_send(
                    &caller_owner,
                    &turn_id,
                    crate::acp::store::SendObservation::CompleteBytes,
                ),
                Err(HostError::RecoveryRequired)
            );
            port.observe_send(
                &caller_owner,
                &turn_id,
                crate::acp::store::SendObservation::CompleteBytes,
            )
            .unwrap();
            let cancel = HostCancelIntent {
                owner: caller_owner.clone(),
                turn_id: turn_id.clone(),
                client_intent_id: ClientIntentId::parse("intent-cancel-turn").unwrap(),
                now_millis: now + 4,
            };
            assert_eq!(
                port.begin_cancel(&cancel),
                Err(HostError::RecoveryRequired)
            );
            port.begin_cancel(&cancel).unwrap();
            port.dispatch_cancel(&cancel).unwrap();
            let close = crate::acp::store::CloseSessionCommand {
                project_id: caller_owner.project_id.clone(),
                chat_id: caller_owner.chat_id.clone(),
                expected_projection_revision: ProjectionRevision::new(9).unwrap(),
                client_intent_id: ClientIntentId::parse("intent-close-turn").unwrap(),
                session_id: caller_owner.session_id.clone(),
                runtime_epoch: caller_owner.runtime_epoch,
                now_millis: now + 5,
            };
            assert_eq!(
                port.begin_close(&close),
                Err(HostError::RecoveryRequired)
            );
            port.begin_close(&close).unwrap();
            assert_eq!(
                port.wait_zero(&caller_owner),
                Err(HostError::RecoveryRequired)
            );
            let proof = port.wait_zero(&caller_owner).unwrap();
            let (error, proof) = port
                .finalize_close(&close, proof)
                .expect_err("the injected projection conflict requires recovery");
            assert_eq!(error, HostError::RecoveryRequired);
            port.finalize_close(&close, proof).unwrap();
            (turn_id, first, retry)
        });

        let first = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .expect("prepare command");
        let ControlMessage::Worker(first) = first else {
            panic!("worker command")
        };
        let mut state = FirstTurnWorkerState::Idle(completed)
            .handle(&context, first)
            .await;
        let FirstTurnWorkerState::Prepared { turn, .. } = &state else {
            panic!("prepared worker state")
        };
        assert_eq!(turn.active_turn.external_turn_id, None);
        assert_eq!(
            turn.active_turn.client_message_id.as_str(),
            turn.dispatch.turn_id().as_str()
        );
        assert_eq!(turn.parts.owner, owner);

        for command_index in 0..15 {
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("worker command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            if matches!(command_index, 0 | 4 | 6 | 9 | 11 | 13) {
                while publisher.take().is_some() {}
                publisher
                    .publish_committed(crate::acp::intent::ProjectionChangedV1 {
                        project_id: project_id.clone(),
                        chat_id: SageChatId::parse("chat-foreign").unwrap(),
                        session_id: started.session_id.clone(),
                        projection_revision: ProjectionRevision::new(1).unwrap(),
                    })
                    .unwrap();
            }
            state = state.handle(&context, command).await;
            if matches!(command_index, 0 | 4 | 6 | 9 | 11 | 13) {
                assert!(matches!(state, FirstTurnWorkerState::Recovery(_)));
                assert!(publisher.take().is_some());
            }
        }
        let (turn_id, first, retry) = caller.join().unwrap();
        assert!(matches!(state, FirstTurnWorkerState::Closed));
        assert_eq!(first, LiveWrite::ZeroBytes);
        assert_eq!(retry, LiveWrite::Complete);
        assert_eq!(cancel_count.load(Ordering::SeqCst), 1);
        assert_eq!(zero_waits.load(Ordering::SeqCst), 1);
        assert!(scheduler.active().is_none());
        let durable_state: (String, String, Option<i64>) = database
            .connection()
            .query_row(
                "SELECT state,delivery_state,terminal_at FROM turns WHERE id=?1",
                rusqlite::params![turn_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (durable_state.0.as_str(), durable_state.1.as_str()),
            ("deliveryUnknown", "completeBytes")
        );
        assert!(durable_state.2.is_some());
        let session_state: String = database
            .connection()
            .query_row(
                "SELECT state FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session_state, "closed");
        let cancel_facts: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM session_facts WHERE turn_id=?1 AND kind='turnCancelIntent'",
                [turn_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cancel_facts, 1);
        let send_facts: (i64, i64) = database
            .connection()
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM session_facts WHERE turn_id=?1 AND kind='sendStarted'),
                   (SELECT COUNT(*) FROM session_facts WHERE turn_id=?1 AND kind='sendCompleteBytes')",
                [turn_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(send_facts, (2, 1));
        let close_facts: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sessionCloseIntent'",
                [started.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(close_facts, 1);

        journal.close_when_drained();
        task.await.unwrap();
        drop(database);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn one_worker_reuses_the_same_session_for_two_store_minted_turns_then_closes() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let root = std::env::temp_dir().join(format!("sage-two-turns-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse("chat-two-turns").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES (?1,?2,1,'Two Turns',0,?3,?3)",
                rusqlite::params![chat_id.as_str(), project_id.as_str(), now],
            )
            .unwrap();
        let publisher = Arc::new(ProjectionMailbox::default());
        let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let task = runner.spawn();
        let scheduler = Scheduler::new();
        let sessions =
            SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
        let initial = crate::acp::store::load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let binding = "55".repeat(32);
        let artifact = "66".repeat(32);
        let started = sessions
            .start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: initial.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-two-start").unwrap(),
                binding_id: RequiredText::new("binding", "binding-two").unwrap(),
                binding_digest: binding.clone(),
                harness_version: None,
                model_id: None,
                now_millis: now,
            })
            .await
            .unwrap();
        let epoch = RuntimeEpoch::new(1).unwrap();
        let lease = sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-two-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: artifact.clone(),
                now_millis: now + 1,
            })
            .await
            .unwrap();
        let external_session = ExternalId::new("external", "external-two").unwrap();
        let mut activation_journal = CoordinatorActivationJournal::new(lease);
        let ready = activation_journal
            .commit_ready(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: binding,
                artifact_digest: artifact,
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities()).unwrap(),
                now_millis: now + 2,
            })
            .await
            .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
        };
        let prompt_ids = Arc::new(Mutex::new(Vec::new()));
        let zero_waits = Arc::new(AtomicUsize::new(0));
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                SequencedAdapter {
                    binding_digest: [0x55; 32],
                    runtime_epoch: epoch,
                    session_id: started.session_id.clone(),
                    external_session_id: external_session.clone(),
                    pending_message: None,
                    prompt_ids: Arc::clone(&prompt_ids),
                    next_request: 0,
                    next_external_turn: 0,
                },
                activation_journal,
                Mapper,
                ActivatedSession {
                    binding_digest: [0x55; 32],
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    external_session_id: external_session,
                    runtime_epoch: epoch,
                    capabilities: capabilities(),
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            transport: Transport(Mutex::new(VecDeque::from([
                FrameWriteOutcome::Complete,
                FrameWriteOutcome::Complete,
            ]))),
            control: Control {
                zero_waits: Arc::clone(&zero_waits),
            },
            owner: owner.clone(),
            clock: Arc::new(SystemDriverClock),
        };
        let authority_state = Arc::new(Mutex::new(IntentSnapshot {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            projection_revision: ready.projection_revision,
            session_id: Some(started.session_id.clone()),
            runtime_epoch: Some(epoch),
            turn_id: None,
            turn_state: None,
            turn_revision: None,
        }));
        let authority = Arc::new(MutableAuthority(Arc::clone(&authority_state)));
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let context = FirstTurnWorkerContext {
            owner: owner.clone(),
            coordinator: SendCoordinator::new(authority, journal.clone(), publisher.clone()),
            journal: journal.clone(),
            publisher,
            clock: Arc::new(SystemDriverClock),
            mailbox: Arc::clone(&mailbox),
            zero_sequence: std::sync::atomic::AtomicU64::new(0),
        };
        let mut state = FirstTurnWorkerState::Idle(completed);

        macro_rules! step {
            ($command:expr) => {{
                let response = mailbox.submit_worker($command).unwrap();
                let message = mailbox
                    .wait_next_until(None, context.clock.as_ref())
                    .unwrap()
                    .expect("worker command");
                let ControlMessage::Worker(envelope) = message else {
                    panic!("worker envelope")
                };
                state = state.handle(&context, envelope).await;
                response.recv().unwrap()
            }};
        }

        let prepare = |intent: &'static str, expected: ProjectionRevision, at: i64| {
            WorkerCommand::PreparePrompt(HostPromptIntent {
                command: PrepareTurnCommand {
                    project_id: project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: expected,
                    client_intent_id: ClientIntentId::parse(intent).unwrap(),
                    prompt: Prompt::new("prompt", format!("prompt-{intent}")).unwrap(),
                    now_millis: at,
                },
            })
        };

        let WorkerReply::Prepared(turn_one) = step!(prepare(
            "intent-turn-one",
            ready.projection_revision,
            now + 3
        ))
        .unwrap()
        else {
            panic!("first prepared reply")
        };
        assert!(matches!(state, FirstTurnWorkerState::Prepared { .. }));
        let WorkerReply::Write(LiveWrite::Complete) = step!(WorkerCommand::DispatchPrompt {
            owner: owner.clone(),
            turn_id: turn_one.clone(),
        })
        .unwrap()
        else {
            panic!("first write")
        };
        step!(WorkerCommand::ObserveSend {
            owner: owner.clone(),
            turn_id: turn_one.clone(),
            observation: crate::acp::store::SendObservation::CompleteBytes,
        })
        .unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        let cursor_one = match state {
            FirstTurnWorkerState::Active(mut active) => {
                active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
                let cursor = active
                    .turn
                    .driver
                    .latest_durable_turn_cursor()
                    .cloned()
                    .expect("first durable terminal");
                state = FirstTurnWorkerState::Active(active);
                cursor
            }
            _ => panic!("first active state"),
        };
        assert_eq!(cursor_one.turn_id(), &turn_one);
        assert_eq!(cursor_one.state(), TurnState::Completed);
        assert_eq!(cursor_one.delivery_state(), DeliveryState::Correlated);
        authority_state.lock().unwrap().projection_revision = cursor_one.projection_revision();
        let terminal_response = mailbox
            .submit_worker(WorkerCommand::AcceptTerminal {
                owner: owner.clone(),
                cursor: cursor_one.clone(),
            })
            .unwrap();
        let queued_response = mailbox.submit_worker(WorkerCommand::Shutdown).unwrap();
        for expected in [&terminal_response, &queued_response] {
            let message = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("queued terminal recovery command");
            let ControlMessage::Worker(envelope) = message else {
                panic!("worker envelope")
            };
            state = state.handle(&context, envelope).await;
            assert!(matches!(
                expected.recv().unwrap(),
                Err(HostError::RecoveryRequired)
            ));
            assert!(matches!(state, FirstTurnWorkerState::Recovery(_)));
        }
        step!(WorkerCommand::AcceptTerminal {
            owner: owner.clone(),
            cursor: cursor_one.clone(),
        })
        .unwrap();
        assert!(matches!(state, FirstTurnWorkerState::Reusable(_)));

        let WorkerReply::Prepared(turn_two) = step!(prepare(
            "intent-turn-two",
            cursor_one.projection_revision(),
            now + 4
        ))
        .unwrap()
        else {
            panic!("second prepared reply")
        };
        assert_ne!(turn_one, turn_two);
        let WorkerReply::Write(LiveWrite::Complete) = step!(WorkerCommand::DispatchPrompt {
            owner: owner.clone(),
            turn_id: turn_two.clone(),
        })
        .unwrap()
        else {
            panic!("second write")
        };
        step!(WorkerCommand::ObserveSend {
            owner: owner.clone(),
            turn_id: turn_two.clone(),
            observation: crate::acp::store::SendObservation::CompleteBytes,
        })
        .unwrap();
        mailbox.try_push_chunk(b"{}\n".to_vec()).unwrap();
        let cursor_two = match state {
            FirstTurnWorkerState::Active(mut active) => {
                active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
                let cursor = active
                    .turn
                    .driver
                    .latest_durable_turn_cursor()
                    .cloned()
                    .expect("second durable terminal");
                state = FirstTurnWorkerState::Active(active);
                cursor
            }
            _ => panic!("second active state"),
        };
        assert_eq!(cursor_two.turn_id(), &turn_two);
        assert_eq!(cursor_two.state(), TurnState::Completed);

        let close = crate::acp::store::CloseSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: cursor_two.projection_revision(),
            client_intent_id: ClientIntentId::parse("intent-two-close").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            now_millis: now + 5,
        };
        step!(WorkerCommand::BeginClose(close.clone())).unwrap();
        let WorkerReply::Zero(proof) = step!(WorkerCommand::WaitZero {
            owner: owner.clone(),
        })
        .unwrap()
        else {
            panic!("zero proof")
        };
        step!(WorkerCommand::FinalizeClose {
            command: close,
            proof,
        })
        .unwrap();
        assert!(matches!(state, FirstTurnWorkerState::Closed));

        assert_eq!(
            *prompt_ids.lock().unwrap(),
            [turn_one.as_str().to_owned(), turn_two.as_str().to_owned()]
        );
        assert_eq!(zero_waits.load(Ordering::SeqCst), 1);
        assert!(scheduler.active().is_none());
        {
            let connection = database.connection();
            let mut statement = connection
                .prepare(
                    "SELECT id,state,delivery_state,terminal_at FROM turns
                     WHERE session_id=?1 ORDER BY ordinal LIMIT 3",
                )
                .unwrap();
            let rows = statement
                .query_map([started.session_id.as_str()], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].0, turn_one.as_str());
            assert_eq!(rows[1].0, turn_two.as_str());
            for row in rows {
                assert_eq!(row.1, "completed");
                assert_eq!(row.2, "correlated");
                assert!(row.3.is_some());
            }
            let facts: (i64, i64) = connection
                .query_row(
                    "SELECT
                       (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sessionCloseIntent'),
                       (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='turnCancelIntent')",
                    [started.session_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(facts, (1, 0));
        }

        journal.close_when_drained();
        task.await.unwrap();
        drop(database);
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn activated_idle_session_closes_without_fabricating_a_turn() {
    close_before_first_write(CloseBeforeWriteCase::Idle);
}

#[test]
fn prepared_turn_closes_as_cancelled_without_writing_prompt_or_cancel() {
    close_before_first_write(CloseBeforeWriteCase::Prepared);
}

#[test]
fn applied_begin_send_commit_unknown_reconciles_without_prompt_replay() {
    close_before_first_write(CloseBeforeWriteCase::BeginSendAppliedUnknown);
}

#[test]
fn absent_begin_send_commit_unknown_reconciles_without_prompt_replay() {
    close_before_first_write(CloseBeforeWriteCase::BeginSendNotAppliedUnknown);
}

fn close_before_first_write(case: CloseBeforeWriteCase) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let root = std::env::temp_dir().join(format!("sage-idle-close-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse("chat-idle-close").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES (?1,?2,1,'Idle Close',0,?3,?3)",
                rusqlite::params![chat_id.as_str(), project_id.as_str(), now],
            )
            .unwrap();
        let publisher = Arc::new(ProjectionMailbox::default());
        let begin_send_injection = Arc::new(Mutex::new(None));
        let (journal, runner) = JournalHandle::new(CommitUnknownOnceStore {
            inner: DatabaseAcpStore::new(database.clone()),
            begin_send: Arc::clone(&begin_send_injection),
        });
        let task = runner.spawn();
        let scheduler = Scheduler::new();
        let sessions =
            SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
        let initial = crate::acp::store::load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let binding = "33".repeat(32);
        let artifact = "44".repeat(32);
        let started = sessions
            .start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: initial.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-idle-start").unwrap(),
                binding_id: RequiredText::new("binding", "binding-idle").unwrap(),
                binding_digest: binding.clone(),
                harness_version: None,
                model_id: None,
                now_millis: now,
            })
            .await
            .unwrap();
        let epoch = RuntimeEpoch::new(1).unwrap();
        let lease = sessions
            .activate_session(ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::parse("intent-idle-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: artifact.clone(),
                now_millis: now + 1,
            })
            .await
            .unwrap();
        let external_session = ExternalId::new("external", "external-idle").unwrap();
        let mut activation_journal = CoordinatorActivationJournal::new(lease);
        let ready = activation_journal
            .commit_ready(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: binding,
                artifact_digest: artifact,
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities()).unwrap(),
                now_millis: now + 2,
            })
            .await
            .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
        };
        let zero_waits = Arc::new(AtomicUsize::new(0));
        let cancel_count = Arc::new(AtomicUsize::new(0));
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                Adapter {
                    cancel_count: Arc::clone(&cancel_count),
                },
                activation_journal,
                Mapper,
                ActivatedSession {
                    // A prepared close must not bind an adapter pump. The zero
                    // digest would make that admission fail deterministically.
                    binding_digest: if case == CloseBeforeWriteCase::Prepared {
                        [0; 32]
                    } else {
                        [0x33; 32]
                    },
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    external_session_id: external_session,
                    runtime_epoch: epoch,
                    capabilities: capabilities(),
                    models: Vec::new(),
                    next_model_cursor: None,
                },
            ),
            transport: Transport(Mutex::new(VecDeque::new())),
            control: Control {
                zero_waits: Arc::clone(&zero_waits),
            },
            owner: owner.clone(),
            clock: Arc::new(SystemDriverClock),
        };
        let authority = Arc::new(Authority(IntentSnapshot {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            projection_revision: ready.projection_revision,
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
            zero_sequence: std::sync::atomic::AtomicU64::new(0),
        };
        let mut state = FirstTurnWorkerState::Idle(completed);
        let expected_projection_revision = if case.prepares_turn() {
            let response = mailbox
                .submit_worker(WorkerCommand::PreparePrompt(HostPromptIntent {
                    command: PrepareTurnCommand {
                        project_id: project_id.clone(),
                        project_generation: ProjectGeneration::ZERO,
                        chat_id: chat_id.clone(),
                        session_id: started.session_id.clone(),
                        runtime_epoch: epoch,
                        expected_projection_revision: ready.projection_revision,
                        client_intent_id: ClientIntentId::parse("intent-prepared-close-turn")
                            .unwrap(),
                        prompt: Prompt::new("prompt", "must-not-write").unwrap(),
                        now_millis: now + 3,
                    },
                }))
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("prepared close command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(
                response.recv().unwrap(),
                Ok(WorkerReply::Prepared(_))
            ));
            assert!(matches!(state, FirstTurnWorkerState::Prepared { .. }));
            crate::acp::store::load_chat_page(
                &database.connection(),
                project_id.clone(),
                chat_id.clone(),
                None,
            )
            .unwrap()
            .projection_revision
        } else {
            ready.projection_revision
        };
        if let Some(applied) = case.begin_send_injection() {
            *begin_send_injection.lock().unwrap() = Some(applied);
            let response = mailbox
                .submit_worker(WorkerCommand::DispatchPrompt {
                    owner: owner.clone(),
                    turn_id: match &state {
                        FirstTurnWorkerState::Prepared { turn, .. } => {
                            turn.dispatch.turn_id().clone()
                        }
                        _ => panic!("prepared state"),
                    },
                })
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("commit-unknown dispatch command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(
                response.recv().unwrap(),
                Err(HostError::RecoveryRequired)
            ));
            assert!(matches!(
                &state,
                FirstTurnWorkerState::Active(active) if active.durable_mutation_unknown()
            ));
        }
        let close = crate::acp::store::CloseSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision,
            client_intent_id: ClientIntentId::parse("intent-idle-close").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
            now_millis: now + 4,
        };
        if case == CloseBeforeWriteCase::Prepared {
            let mut foreign = close.clone();
            foreign.runtime_epoch = RuntimeEpoch::new(2).unwrap();
            let response = mailbox
                .submit_worker(WorkerCommand::BeginClose(foreign))
                .unwrap();
            let command = mailbox
                .wait_next_until(None, context.clock.as_ref())
                .unwrap()
                .expect("foreign prepared close command");
            let ControlMessage::Worker(command) = command else {
                panic!("worker command")
            };
            state = state.handle(&context, command).await;
            assert!(matches!(response.recv().unwrap(), Err(HostError::Intent)));
            assert!(matches!(state, FirstTurnWorkerState::Prepared { .. }));
        }
        let response = mailbox
            .submit_worker(WorkerCommand::BeginClose(close.clone()))
            .unwrap();
        let command = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .expect("begin close command");
        let ControlMessage::Worker(command) = command else {
            panic!("worker command")
        };
        state = state.handle(&context, command).await;
        match response.recv().unwrap() {
            Ok(WorkerReply::Unit) => {}
            Err(error) => match &state {
                FirstTurnWorkerState::Recovery(recovery) => match recovery.as_ref() {
                    FirstTurnWorkerRecovery::BeginClose(failure) => {
                        panic!("begin close failed: {error:?}: {:?}", failure.error)
                    }
                    _ => panic!("begin close failed in another recovery: {error:?}"),
                },
                _ => panic!("begin close failed without recovery: {error:?}"),
            },
            _ => panic!("begin close reply"),
        }

        let response = mailbox
            .submit_worker(WorkerCommand::WaitZero {
                owner: owner.clone(),
            })
            .unwrap();
        let command = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .expect("wait zero command");
        let ControlMessage::Worker(command) = command else {
            panic!("worker command")
        };
        state = state.handle(&context, command).await;
        let Ok(WorkerReply::Zero(proof)) = response.recv().unwrap() else {
            panic!("zero proof")
        };

        let response = mailbox
            .submit_worker(WorkerCommand::FinalizeClose {
                command: close.clone(),
                proof,
            })
            .unwrap();
        let command = mailbox
            .wait_next_until(None, context.clock.as_ref())
            .unwrap()
            .expect("finalize close command");
        let ControlMessage::Worker(command) = command else {
            panic!("worker command")
        };
        state = state.handle(&context, command).await;
        match response.recv().unwrap() {
            Ok(WorkerReply::Unit) => {}
            Err(error) => match &state {
                FirstTurnWorkerState::Recovery(recovery) => match recovery.as_ref() {
                    FirstTurnWorkerRecovery::Finalize(failure) => {
                        panic!("finalize close failed: {error:?}: {:?}", failure.error)
                    }
                    _ => panic!("finalize close failed in another recovery: {error:?}"),
                },
                _ => panic!("finalize close failed without recovery: {error:?}"),
            },
            _ => panic!("finalize close reply"),
        }
        assert!(matches!(state, FirstTurnWorkerState::Closed));
        assert_eq!(zero_waits.load(Ordering::SeqCst), 1);
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
        assert!(scheduler.active().is_none());
        let session: (String, String) = database
            .connection()
            .query_row(
                "SELECT state,residency FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(session, ("closed".to_owned(), "cold".to_owned()));
        let epoch_state: (String, Option<String>) = database
            .connection()
            .query_row(
                "SELECT state,exit_kind FROM session_runtime_epochs WHERE session_id=?1 AND runtime_epoch=?2",
                rusqlite::params![started.session_id.as_str(), epoch.get() as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(epoch_state, ("exited".to_owned(), Some("userClose".to_owned())));
        let counts: (i64, i64, i64, i64) = database
            .connection()
            .query_row(
                "SELECT
                   (SELECT COUNT(*) FROM turns WHERE session_id=?1),
                   (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sessionCloseIntent'),
                   (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='closeFinalized'),
                   (SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='turnCancelIntent')",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(counts, (i64::from(case.prepares_turn()), 1, 1, 0));
        if case.prepares_turn() {
            let turn: (String, String) = database
                .connection()
                .query_row(
                    "SELECT state,delivery_state FROM turns WHERE session_id=?1",
                    [started.session_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            let expected = match case {
                CloseBeforeWriteCase::BeginSendAppliedUnknown => {
                    ("deliveryUnknown".to_owned(), "possibleBytes".to_owned())
                }
                _ => ("cancelled".to_owned(), "notWritten".to_owned()),
            };
            assert_eq!(turn, expected);
        }

        let send_started: i64 = database
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM session_facts WHERE session_id=?1 AND kind='sendStarted'",
                [started.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            send_started,
            i64::from(case == CloseBeforeWriteCase::BeginSendAppliedUnknown)
        );

        journal.close_when_drained();
        task.await.unwrap();
        drop(database);
        fs::remove_dir_all(root).unwrap();
    });
}

fn capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        config_count: 0,
    }
}
