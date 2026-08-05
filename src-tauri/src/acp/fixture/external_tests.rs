use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use uuid::Uuid;

use crate::{
    acp::{
        contract::{
            ClientIntentId, ProjectGeneration, RequiredText, RuntimeEpoch, SafeText, SageChatId,
            SageProjectId,
        },
        fixture::adapter::FixtureAdapter,
        intent::{ExternalId, IntentSnapshot, Prompt},
        lifecycle::{DeliveryState, ItemLifecycle, TurnState},
        process::{PlatformSupervisor, TreeCapacityPool},
        projection::ItemContent,
        recovery::recover_after_restart,
        runtime::{
            ActivatedSession, ActivationJournal, ActiveCloseBeginState, BlockingStdoutReader,
            CompletedActivationDriverParts, CoordinatorActivationJournal, DriverClock,
            DriverMailbox, IdleActivationParts, JournalHandle, LiveWrite, PreparedProcessWrite,
            SendCoordinator, SessionCoordinator, SystemDriverClock,
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

use super::external_fixture_adapters::{
    Authority, CommitCheckingPublisher, ExternalCase, Mapper, capabilities, fixture_artifact, hex,
    launch_spec, send_started,
};

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn turn_text_success_runs_through_supervisor_driver_and_sqlite() {
    run_external_case(ExternalCase::TurnTextSuccess);
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn complete_delivery_exit_cases_recover_without_new_send() {
    for case_id in [
        "send-complete-uncorrelated-exit",
        "send-correlated-then-exit",
    ] {
        run_external_case(ExternalCase::CompleteDeliveryExit(case_id));
    }
}

fn run_external_case(case: ExternalCase) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
    let case_id = case.id();
    let (fixture_path, artifact_digest, fixture_byte_len) = fixture_artifact();
    let artifact_hex = hex(&artifact_digest);
    let binding_digest = [7_u8; 32];
    let binding_hex = hex(&binding_digest);

    let root = std::env::temp_dir().join(format!("sage-fixture-e2e-{}", Uuid::new_v4()));
    let project_root = root.join("project");
    fs::create_dir_all(&project_root).unwrap();
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
    drop(sends);
    drop(sessions);
    let (active, write) = active
        .dispatch_once(now + 4)
        .await
        .unwrap_or_else(|failure| panic!("dispatch fixture prompt: {:?}", failure.error));
    assert_eq!(write, LiveWrite::Complete);
    let mut active = active
        .observe(SendObservation::CompleteBytes, now + 5)
        .await
        .unwrap_or_else(|failure| panic!("observe fixture prompt: {:?}", failure.error));

    if matches!(case, ExternalCase::CompleteDeliveryExit(_)) {
        let mut teardown_observed = false;
        for _ in 0..4 {
            let wake = tokio::time::timeout(
                Duration::from_secs(5),
                active.turn.driver.run_next(mailbox.as_ref()),
            )
            .await
            .expect("delivery fixture driver wake timed out");
            match wake {
                Ok(_) => {}
                Err(crate::acp::runtime::DriverError::TeardownRequired(_)) => {
                    teardown_observed = true;
                    break;
                }
                Err(error) => panic!("delivery fixture driver failed unexpectedly: {error:?}"),
            }
        }
        assert!(teardown_observed, "delivery fixture did not exit in four wakes");
        let correlated = active
            .turn
            .driver
            .latest_durable_turn_cursor()
            .is_some_and(|cursor| cursor.delivery_state() == DeliveryState::Correlated);
        let _ = retained_control.wait_zero().unwrap();
        reader.join().unwrap();
        assert_eq!(process_capacity.usage(), (0, 0));
        let session_id = started.session_id.clone();
        let before = send_started(&database, &session_id);
        drop(active);
        drop(scheduler);
        drop(publisher);
        drop(mailbox);
        drop(retained_control);
        journal.close_when_drained();
        journal_task.await.unwrap();
        drop(journal);
        drop(database);
        let reopened = Database::open(&database_path).unwrap();
        let report =
            recover_after_restart(&mut reopened.connection(), crate::persistence::now_millis())
                .unwrap();
        assert_eq!(report.recovered_sessions, 1);
        let after = send_started(&reopened, &session_id);
        let automatic_replay = after > before;
        assert_eq!(after, before);
        let (state, delivery): (String, String) = reopened
            .connection()
            .query_row(
                "SELECT state,delivery_state FROM turns WHERE session_id=?1",
                [session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let expected = if correlated {
            ("interrupted", "correlated")
        } else {
            ("deliveryUnknown", "completeBytes")
        };
        assert_eq!((state.as_str(), delivery.as_str()), expected);
        let case = super::generated_cases::FROZEN_CASES
            .iter()
            .find(|case| case.id == case_id)
            .unwrap();
        super::oracle::assert_literal(
            case,
            if correlated {
                serde_json::json!({"turn":"interrupted","deliveryUnknown":false})
            } else {
                serde_json::json!({
                    "turn":"deliveryUnknown",
                    "automaticReplay":automatic_replay,
                })
            },
        );
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
        return;
    }

    while active
        .turn
        .driver
        .latest_durable_turn_cursor()
        .is_none_or(|cursor| !cursor.state().is_terminal())
    {
        active.turn.driver.run_next(mailbox.as_ref()).await.unwrap();
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
    drop(scheduler);
    drop(publisher);
    drop(mailbox);
    drop(retained_control);
    drop(database);
    fs::remove_dir_all(root).unwrap();
    });
}
