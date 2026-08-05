use std::{path::PathBuf, sync::Arc};

use crate::{
    acp::{
        contract::{
            ClientIntentId, ProjectGeneration, RequiredText, RuntimeEpoch, SafeText, SageChatId,
            SageProjectId,
        },
        fixture::adapter::FixtureAdapter,
        intent::{ExternalId, IntentSnapshot, Prompt},
        process::{
            PlatformSupervisor, StdinEndpoint, StdoutEndpoint, TreeCapacityPool, TreeControl,
            TreeExit,
        },
        runtime::{
            ActivatedSession, ActivationJournal, BlockingStdoutReader,
            CompletedActivationDriverParts, CoordinatorActivationJournal, DriverClock,
            DriverMailbox, HostPromptIntent, IdleActivationParts, JournalHandle,
            PreparedProcessWrite, SendCoordinator, SessionCoordinator, SqliteAdapterBatchJournal,
            SystemDriverClock,
        },
        scheduler::{RuntimeOwner, Scheduler},
        store::{
            ActivateSessionCommand, CompleteActivationCommand, DatabaseAcpStore,
            PrepareTurnCommand, SessionMutation, StartSessionCommand, load_chat_page,
        },
    },
    persistence::Database,
};

use super::external_fixture_adapters::{
    Authority, CommitCheckingPublisher, ExternalCase, Mapper, capabilities, fixture_artifact, hex,
    launch_spec,
};

pub(super) type ExternalActive = PreparedProcessWrite<
    FixtureAdapter,
    SqliteAdapterBatchJournal,
    Mapper,
    StdinEndpoint,
    TreeControl,
>;

pub(super) struct ExternalPrepared {
    pub(super) case_id: &'static str,
    pub(super) root: PathBuf,
    pub(super) database_path: PathBuf,
    pub(super) database: Database,
    pub(super) project_id: SageProjectId,
    pub(super) chat_id: SageChatId,
    pub(super) started: SessionMutation,
    pub(super) epoch: RuntimeEpoch,
    pub(super) now_millis: i64,
    pub(super) prompt_intent: HostPromptIntent,
    pub(super) publisher: Arc<CommitCheckingPublisher>,
    pub(super) journal: JournalHandle,
    pub(super) journal_task: tokio::task::JoinHandle<()>,
    pub(super) scheduler: Scheduler,
    pub(super) process_capacity: Arc<TreeCapacityPool>,
    pub(super) retained_control: TreeControl,
    pub(super) mailbox: Arc<DriverMailbox>,
    pub(super) reader: Option<std::thread::JoinHandle<()>>,
    pub(super) active: ExternalActive,
}

pub(super) async fn prepare_external_case(case: ExternalCase) -> ExternalPrepared {
    let case_id = case.id();
    let (fixture_path, artifact_digest, fixture_byte_len) = fixture_artifact();
    let artifact_hex = hex(&artifact_digest);
    let binding_digest = [7_u8; 32];
    let binding_hex = hex(&binding_digest);
    let root = std::env::temp_dir().join(format!("sage-fixture-e2e-{}", uuid::Uuid::new_v4()));
    let project_root = root.join("project");
    std::fs::create_dir_all(&project_root).unwrap();
    let database_path = root.join("sage.db");
    let database = Database::open(&database_path).unwrap();
    let project = database.create_project_from_root(&project_root).unwrap();
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
        terminal_seen_after_commit: std::sync::atomic::AtomicBool::new(false),
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
    let spec = launch_spec(
        case_id,
        epoch,
        fixture_path,
        artifact_digest,
        fixture_byte_len,
    );
    let process_capacity = TreeCapacityPool::isolated_ade_0006a();
    let tree = PlatformSupervisor::launch(
        &spec,
        process_capacity
            .try_acquire(spec.limits.descendants)
            .unwrap(),
    )
    .unwrap();
    let (stdin, stdout, control) = tree.into_parts();
    let retained_control = control.clone();
    let mailbox = Arc::new(DriverMailbox::new().unwrap());
    let reader = if matches!(case, ExternalCase::ZeroByteFailure) {
        await_stdin_closed_barrier(stdout, &retained_control);
        None
    } else {
        let reader_mailbox = Arc::clone(&mailbox);
        Some(std::thread::spawn(move || {
            BlockingStdoutReader::new(stdout).run(reader_mailbox.as_ref());
        }))
    };
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
        owner,
        clock: Arc::new(SystemDriverClock) as Arc<dyn DriverClock>,
    };
    let sends = SendCoordinator::new(
        Arc::new(Authority(IntentSnapshot {
            project_id: project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: chat_id.clone(),
            projection_revision: ready.projection_revision,
            session_id: Some(started.session_id.clone()),
            runtime_epoch: Some(epoch),
            turn_id: None,
            turn_state: None,
            turn_revision: None,
        })),
        journal.clone(),
        publisher.clone(),
    );
    let prompt = Prompt::new("prompt", "read only").unwrap();
    let prompt_command = PrepareTurnCommand {
        project_id: project_id.clone(),
        project_generation: ProjectGeneration::ZERO,
        chat_id: chat_id.clone(),
        session_id: started.session_id.clone(),
        runtime_epoch: epoch,
        expected_projection_revision: ready.projection_revision,
        client_intent_id: ClientIntentId::parse("fixture-turn").unwrap(),
        prompt: prompt.clone(),
        now_millis: now + 3,
    };
    let prompt_intent = HostPromptIntent {
        command: prompt_command.clone(),
    };
    let prepared = completed
        .prepare_first_turn(&sends, prompt_command)
        .await
        .unwrap_or_else(|_| panic!("prepare first fixture Turn"));
    let running = prepared
        .bind_driver(journal.clone(), publisher.clone())
        .unwrap_or_else(|_| panic!("bind fixture driver"));
    drop(sends);
    drop(sessions);
    ExternalPrepared {
        case_id,
        root,
        database_path,
        database,
        project_id,
        chat_id,
        started,
        epoch,
        now_millis: now + 4,
        prompt_intent,
        publisher,
        journal,
        journal_task,
        scheduler,
        process_capacity,
        retained_control,
        mailbox,
        reader,
        active: PreparedProcessWrite::new(running, prompt),
    }
}

fn await_stdin_closed_barrier(mut stdout: StdoutEndpoint, control: &TreeControl) {
    let exit = match control.wait_zero() {
        Ok((exit, _proof)) => exit,
        Err(error) => {
            control
                .terminate_zero(TreeExit::Deadline)
                .unwrap_or_else(|force_error| {
                    panic!(
                        "native stdin-close barrier failed to converge after {error:?}: {force_error:?}"
                    )
                });
            panic!("native stdin-close barrier process did not converge: {error:?}");
        }
    };
    assert!(matches!(exit, TreeExit::Exited { code: Some(8) }));
    // Zero-process convergence closes every inherited writer, so this bounded
    // frame decoder can consume buffered output without an unbounded pipe wait.
    let frame = stdout
        .read_frame()
        .unwrap()
        .expect("native stdin-close barrier");
    let barrier: serde_json::Value = serde_json::from_slice(frame.as_bytes()).unwrap();
    assert_eq!(
        barrier.get("method").and_then(serde_json::Value::as_str),
        Some("fixture/stdinClosedBarrier")
    );
}
