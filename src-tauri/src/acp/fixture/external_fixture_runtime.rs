use std::{path::PathBuf, sync::Arc};

use crate::{
    acp::{
        contract::{ClientIntentId, ProjectGeneration, RuntimeEpoch, SageChatId, SageProjectId},
        fixture::adapter::FixtureAdapter,
        intent::{ExternalId, IntentSnapshot, Prompt},
        process::{
            PlatformSupervisor, StdinEndpoint, StdoutEndpoint, TreeCapacityPool, TreeControl,
            TreeExit,
        },
        runtime::{
            ActivatedSession, ActivationJournal, BlockingStdoutReader, BoundedThread,
            CompletedActivationDriverParts, CoordinatorActivationJournal, DriverClock,
            DriverMailbox, HostPromptIntent, IdleActivationParts, JournalHandle,
            PreparedProcessWrite, SendCoordinator, SqliteAdapterBatchJournal, SystemDriverClock,
        },
        scheduler::{RuntimeOwner, Scheduler},
        store::{
            ActivateSessionCommand, CompleteActivationCommand, PrepareTurnCommand, SessionMutation,
        },
    },
    persistence::Database,
};

use super::external_fixture_adapters::{
    Authority, CommitCheckingPublisher, ExternalCase, Mapper, fixture_artifact, hex, launch_spec,
};
use super::external_fixture_support::{
    FIXTURE_BINDING_DIGEST, FixtureEnvironment, ProcessCleanup, TempRoot,
    prepare_fixture_environment,
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
    pub(super) process_cleanup: ProcessCleanup,
    pub(super) mailbox: Arc<DriverMailbox>,
    pub(super) active: ExternalActive,
    pub(super) temp_root: TempRoot,
}

pub(super) async fn prepare_external_case(case: ExternalCase) -> ExternalPrepared {
    let case_id = case.id();
    let (fixture_path, artifact_digest, fixture_byte_len) = fixture_artifact();
    let artifact_hex = hex(&artifact_digest);
    let binding_digest = FIXTURE_BINDING_DIGEST;
    let binding_hex = hex(&binding_digest);
    let FixtureEnvironment {
        temp_root,
        database_path,
        database,
        project_id,
        chat_id,
        now_millis: now,
        publisher,
        journal,
        journal_task,
        scheduler,
        sessions,
        started,
        ..
    } = prepare_fixture_environment(
        "sage-fixture-e2e",
        "fixture-chat",
        "Fixture",
        "fixture-start",
    )
    .await;
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
    let capabilities = FixtureAdapter::activation_capabilities();
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
    let mailbox = Arc::new(DriverMailbox::new().unwrap());
    let mut process_cleanup = ProcessCleanup::new(control.clone(), None);
    if matches!(case, ExternalCase::ZeroByteFailure) {
        await_stdin_closed_barrier(stdout, &control);
        process_cleanup.finish_after_zero().unwrap();
    } else {
        let reader_mailbox = Arc::clone(&mailbox);
        let reader = BoundedThread::spawn("external-fixture-reader", move || {
            BlockingStdoutReader::new(stdout).run(reader_mailbox.as_ref());
        })
        .unwrap();
        process_cleanup.attach_reader(reader);
    }
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
        process_cleanup,
        mailbox,
        active: PreparedProcessWrite::new(running, prompt),
        temp_root,
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
