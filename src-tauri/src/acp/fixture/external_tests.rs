use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    acp::{
        bindings::adapter::ResourceMapper,
        contract::{
            ClientIntentId, ProjectGeneration, RequiredText, RuntimeEpoch, SafeText, SageChatId,
            SageProjectId, SchemaVersionV1,
        },
        fixture::adapter::FixtureAdapter,
        intent::{
            CapabilitySnapshotV1, ExternalId, IntentSnapshot, ProjectionChangedV1, Prompt,
            SafeResourceRef, UnavailableResourceReason,
        },
        lifecycle::{ItemLifecycle, TurnState},
        process::{
            ArtifactExpectation, BoundedEnvironment, BoundedOsArgs, CanonicalNativeDirectory,
            LaunchSpec, PlatformSupervisor, ProcessDeadlines, RequiredContainment,
            TreeCapacityPool, TreeLimits,
        },
        projection::ItemContent,
        runtime::{
            ActivatedSession, ActivationJournal, ActiveCloseBeginState, BlockingStdoutReader,
            CompletedActivationDriverParts, CoordinatorActivationJournal, DriverClock,
            DriverMailbox, IdleActivationParts, JournalHandle, LiveWrite, PreparedProcessWrite,
            ProjectIntentAuthority, ProjectionPublishError, ProjectionPublishStatus,
            ProjectionPublisher, SendCoordinator, SessionCoordinator, SystemDriverClock,
        },
        scheduler::{RuntimeOwner, Scheduler},
        store::{
            ActivateSessionCommand, CloseSessionCommand, CompleteActivationCommand,
            DatabaseAcpStore, PrepareTurnCommand, SendObservation, StartSessionCommand,
            load_chat_page,
        },
    },
    persistence::Database,
};

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}

struct Authority(IntentSnapshot);

impl ProjectIntentAuthority for Authority {
    fn current_snapshot(&self, _command: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.clone())
    }
}

struct CommitCheckingPublisher {
    database: Database,
    terminal_seen_after_commit: AtomicBool,
}

impl ProjectionPublisher for CommitCheckingPublisher {
    fn publish_committed(
        &self,
        event: ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        let connection = self.database.connection();
        let revision: i64 = connection
            .query_row(
                "SELECT projection_revision FROM chats WHERE id=?1 AND project_id=?2",
                rusqlite::params![event.chat_id.as_str(), event.project_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(revision >= event.projection_revision.get() as i64);
        let completed: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM turns WHERE session_id=?1 AND state='completed'",
                [event.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        if completed > 0 {
            let items: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM items
                     WHERE session_id=?1 AND lifecycle='completed'
                       AND payload_json LIKE '%fixture text%'",
                    [event.session_id.as_str()],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(items, 1);
            self.terminal_seen_after_commit
                .store(true, Ordering::SeqCst);
        }
        Ok(ProjectionPublishStatus::Published)
    }
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn turn_text_success_runs_through_supervisor_driver_and_sqlite() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let fixture_path = PathBuf::from(
            std::env::var_os("SAGE_ACP_FIXTURE_PATH")
                .expect("set SAGE_ACP_FIXTURE_PATH to the isolated fixture executable"),
        )
        .canonicalize()
        .unwrap();
        let fixture_bytes = fs::read(&fixture_path).unwrap();
        let artifact_digest: [u8; 32] = Sha256::digest(&fixture_bytes).into();
        let artifact_hex = hex(&artifact_digest);
        let binding_digest = [7_u8; 32];
        let binding_hex = hex(&binding_digest);

        let root = std::env::temp_dir().join(format!("sage-fixture-e2e-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse("fixture-chat").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES (?1,?2,1,'Fixture',0,?3,?3)",
                rusqlite::params![chat_id.as_str(), project_id.as_str(), now],
            )
            .unwrap();

        let publisher = Arc::new(CommitCheckingPublisher {
            database: database.clone(),
            terminal_seen_after_commit: AtomicBool::new(false),
        });
        let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let journal_task = runner.spawn();
        let scheduler = Scheduler::new();
        let sessions = SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher.clone());
        let initial = load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let started = sessions
            .start_session(StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: initial.projection_revision,
                client_intent_id: ClientIntentId::parse("fixture-start").unwrap(),
                binding_id: RequiredText::new("binding", "fixture-binding").unwrap(),
                binding_digest: binding_hex.clone(),
                harness_version: Some(SafeText::new("harnessVersion", "fixture-1").unwrap()),
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
                client_intent_id: ClientIntentId::parse("fixture-activate").unwrap(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                artifact_digest: artifact_hex.clone(),
                now_millis: now + 1,
            })
            .await
            .unwrap();
        let capabilities = capabilities();
        let external_session = ExternalId::new("external", "fixture-external").unwrap();
        let mut activation_journal = CoordinatorActivationJournal::new(lease);
        let ready = activation_journal
            .commit_ready(CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: epoch,
                binding_digest: binding_hex,
                artifact_digest: artifact_hex,
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities).unwrap(),
                now_millis: now + 2,
            })
            .await
            .unwrap();

        let spec = LaunchSpec {
            binding_id: Uuid::nil(),
            runtime_epoch: epoch.get(),
            artifact: ArtifactExpectation::new(
                Uuid::nil(),
                fixture_path,
                artifact_digest,
                fixture_bytes.len() as u64,
            )
            .unwrap(),
            fixed_args: BoundedOsArgs::new(vec![
                OsString::from("--case"),
                OsString::from("turn-text-success"),
                OsString::from("--seed"),
                OsString::from("0"),
            ])
            .unwrap(),
            environment: BoundedEnvironment::new(Vec::new()).unwrap(),
            working_directory: CanonicalNativeDirectory::new(
                std::env::current_dir().unwrap().canonicalize().unwrap(),
            )
            .unwrap(),
            required_guarantee: RequiredContainment::EnforcedTree,
            limits: TreeLimits::ade_0006a(),
            deadlines: ProcessDeadlines::ade_0006a(),
        };
        let process_capacity = TreeCapacityPool::isolated_ade_0006a();
        let tree = PlatformSupervisor::launch(
            &spec,
            process_capacity
                .try_acquire(spec.limits.descendants)
                .unwrap(),
        )
        .unwrap();
        let (stdin, stdout, control) = tree.into_parts();
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let reader_mailbox = Arc::clone(&mailbox);
        let reader = std::thread::spawn(move || {
            BlockingStdoutReader::new(stdout).run(reader_mailbox.as_ref());
        });

        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: epoch,
        };
        let completed = CompletedActivationDriverParts {
            parts: IdleActivationParts::new(
                FixtureAdapter::new(
                    binding_digest,
                    epoch,
                    started.session_id.clone(),
                    external_session.clone(),
                )
                .unwrap(),
                activation_journal,
                Mapper,
                ActivatedSession {
                    binding_digest,
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
            transport: stdin,
            control,
            owner: owner.clone(),
            clock: Arc::new(SystemDriverClock) as Arc<dyn DriverClock>,
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
        let prompt = Prompt::new("prompt", "read only").unwrap();
        let prepared = completed
            .prepare_first_turn(
                &sends,
                PrepareTurnCommand {
                    project_id: project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: epoch,
                    expected_projection_revision: ready.projection_revision,
                    client_intent_id: ClientIntentId::parse("fixture-turn").unwrap(),
                    prompt: prompt.clone(),
                    now_millis: now + 3,
                },
            )
            .await
            .unwrap_or_else(|_| panic!("prepare first fixture Turn"));
        let running = prepared
            .bind_driver(journal.clone(), publisher.clone())
            .unwrap_or_else(|_| panic!("bind fixture driver"));
        let active = PreparedProcessWrite::new(running, prompt);
        let (active, write) = active
            .dispatch_once(now + 4)
            .await
            .unwrap_or_else(|failure| panic!("dispatch fixture prompt: {:?}", failure.error));
        assert_eq!(write, LiveWrite::Complete);
        let mut active = active
            .observe(SendObservation::CompleteBytes, now + 5)
            .await
            .unwrap_or_else(|failure| panic!("observe fixture prompt: {:?}", failure.error));

        while active
            .turn
            .driver
            .latest_durable_turn_cursor()
            .is_none_or(|cursor| !cursor.state().is_terminal())
        {
            active
                .turn
                .driver
                .run_next(mailbox.as_ref())
                .await
                .unwrap();
        }
        reader.join().unwrap();
        assert!(publisher.terminal_seen_after_commit.load(Ordering::SeqCst));

        let page = load_chat_page(
            &database.connection(),
            project_id.clone(),
            chat_id.clone(),
            None,
        )
        .unwrap();
        let turn = page.turns.as_slice().last().unwrap();
        assert_eq!(turn.state, TurnState::Completed);
        assert_eq!(turn.items.as_slice().len(), 2);
        let assistant = turn.items.as_slice().last().unwrap();
        assert_eq!(assistant.lifecycle, ItemLifecycle::Completed);
        assert!(matches!(
            &assistant.content,
            ItemContent::Text { text, .. } if text.as_str() == "fixture text"
        ));
        let fixture_case = super::generated_cases::FROZEN_CASES
            .iter()
            .find(|case| case.id == "turn-text-success")
            .unwrap();
        super::oracle::assert_literal(
            fixture_case,
            serde_json::json!({
                "turn": if turn.state == TurnState::Completed {
                    "completed"
                } else {
                    "incomplete"
                },
                "item": if assistant.lifecycle == ItemLifecycle::Completed {
                    "completed"
                } else {
                    "incomplete"
                },
                "commitBeforeNotify": publisher
                    .terminal_seen_after_commit
                    .load(Ordering::SeqCst),
            }),
        );

        let close = CloseSessionCommand {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: page.projection_revision,
            client_intent_id: ClientIntentId::parse("fixture-close").unwrap(),
            session_id: started.session_id,
            runtime_epoch: epoch,
            now_millis: now + 6,
        };
        let closing = ActiveCloseBeginState::new(active, close.clone())
            .begin()
            .await
            .unwrap_or_else(|_| panic!("begin fixture close"));
        let zero = closing
            .converge_zero(1, now + 7)
            .await
            .unwrap_or_else(|_| panic!("converge fixture process tree"));
        let receipt = zero.receipt().clone();
        let closed = zero
            .finalize(&close, &receipt)
            .await
            .unwrap_or_else(|_| panic!("finalize fixture close"));
        drop(closed);
        assert!(scheduler.active().is_none());
        assert_eq!(process_capacity.usage(), (0, 0));

        journal.close_when_drained();
        journal_task.await.unwrap();
        drop(database);
        fs::remove_dir_all(root).unwrap();
    });
}

fn capabilities() -> CapabilitySnapshotV1 {
    CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "fixture-1").unwrap(),
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

fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
