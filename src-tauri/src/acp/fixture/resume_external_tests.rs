use std::{sync::Arc, time::Duration};

use crate::{
    acp::{
        contract::{
            ClientIntentId, RequiredText, RuntimeEpoch, SafeText, SageChatId, SageProjectId,
        },
        fixture::adapter::FixtureAdapter,
        intent::ExternalId,
        lifecycle::SessionState,
        process::{PlatformSupervisor, TreeCapacityPool},
        runtime::{
            ActivationDriverError, ActivationIntent, ActivationMode, ActivationProcessDriver,
            ActivationPump, BoundedThread, CoordinatorActivationHandoff,
            CoordinatorActivationJournal, DriverClock, DriverMailbox, JournalHandle,
            ProjectionPublisher, SessionCoordinator, SystemDriverClock, ZeroProcessProof,
        },
        scheduler::{RuntimeOwner, Scheduler},
        store::{
            DatabaseAcpStore, FailActivationCommand, ParkSessionCommand, StartSessionCommand,
            load_chat_page,
        },
    },
    persistence::Database,
};

use super::{
    external_fixture_adapters::{
        CommitCheckingPublisher, Mapper, fixture_artifact, hex, launch_spec,
    },
    generated_cases::FROZEN_CASES,
};

mod setup;
mod support;

use setup::{
    activate_new, activation_command, historical_turns, persisted_capabilities,
    prove_prior_zero_process, seed_historical_turn,
};
use support::{ObservedStdin, ProcessCleanup, ResumeCase, TempRoot};

const EXTERNAL_SESSION: &str = "fixture-persisted-session-1";

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn resume_cases_run_from_cold_through_supervisor_driver_and_sqlite() {
    for case in [ResumeCase::Success, ResumeCase::Failure] {
        run_resume_case(case);
    }
}

fn run_resume_case(case: ResumeCase) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let temp_root = TempRoot::new("sage-resume-e2e");
        let root = temp_root.path().to_path_buf();
        let project_root = root.join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let database = Database::open(&root.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&project_root).unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse("fixture-resume-chat").unwrap();
        let now = crate::persistence::now_millis();
        database
            .connection()
            .execute(
                "INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at)
                 VALUES (?1,?2,1,'Resume fixture',0,?3,?3)",
                rusqlite::params![chat_id.as_str(), project_id.as_str(), now],
            )
            .unwrap();

        let publisher: Arc<dyn ProjectionPublisher> = Arc::new(CommitCheckingPublisher {
            database: database.clone(),
            terminal_seen_after_commit: Default::default(),
        });
        let (journal, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let journal_task = runner.spawn();
        let scheduler = Scheduler::new();
        let sessions = SessionCoordinator::new(journal.clone(), scheduler.clone(), publisher);
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
                client_intent_id: ClientIntentId::parse("resume-fixture-start").unwrap(),
                binding_id: RequiredText::new("binding", "fixture-binding").unwrap(),
                binding_digest: hex(&[7_u8; 32]),
                harness_version: Some(SafeText::new("harnessVersion", "fixture-1").unwrap()),
                model_id: None,
                now_millis: now,
            })
            .await
            .unwrap();

        let (fixture_path, artifact_digest, fixture_byte_len) = fixture_artifact();
        let artifact_hex = hex(&artifact_digest);
        let binding_digest = [7_u8; 32];
        let binding_hex = hex(&binding_digest);
        let external_session = ExternalId::new("externalSessionId", EXTERNAL_SESSION).unwrap();
        let capabilities = FixtureAdapter::activation_capabilities();
        let first_epoch = RuntimeEpoch::new(1).unwrap();
        let first_owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: first_epoch,
        };
        let mut first_lease = activate_new(
            &sessions,
            &first_owner,
            started.projection_revision,
            &project_root,
            external_session.clone(),
            &artifact_hex,
            now + 1,
        )
        .await;
        let capabilities_before = persisted_capabilities(&database, &started.session_id);
        let history_before = seed_historical_turn(
            &database,
            &project_id,
            &chat_id,
            &started.session_id,
            first_epoch,
            now + 4,
        );

        let (first_capacity, first_proof) = prove_prior_zero_process(
            case.id(),
            first_epoch,
            fixture_path.clone(),
            artifact_digest,
            fixture_byte_len,
            first_owner,
        );
        let parked = first_lease
            .park_after_zero_process(
                ParkSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: first_epoch,
                    exit_kind: RequiredText::new("exitKind", "idleAfterTurn").unwrap(),
                    now_millis: now + 7,
                },
                first_proof,
            )
            .await
            .unwrap();
        drop(first_lease);
        let prior: (String, String) = database
            .connection()
            .query_row(
                "SELECT state,residency FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let zero_before_activation = first_capacity.usage() == (0, 0)
            && scheduler.active().is_none()
            && prior == ("ready".into(), "cold".into());

        let second_epoch = RuntimeEpoch::new(2).unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: second_epoch,
        };
        let second_lease = sessions
            .activate_session(activation_command(
                &owner,
                parked.projection_revision,
                "resume-fixture-activate-2",
                &artifact_hex,
                now + 8,
            ))
            .await
            .unwrap();
        let second_spec = launch_spec(
            case.id(),
            second_epoch,
            fixture_path,
            artifact_digest,
            fixture_byte_len,
        );
        let second_capacity = TreeCapacityPool::isolated_ade_0006a();
        let second_tree = PlatformSupervisor::launch(
            &second_spec,
            second_capacity
                .try_acquire(second_spec.limits.descendants)
                .unwrap(),
        )
        .unwrap();
        let (stdin, stdout, control) = second_tree.into_parts();
        let mailbox = Arc::new(DriverMailbox::new().unwrap());
        let reader_mailbox = Arc::clone(&mailbox);
        let mut process_cleanup = ProcessCleanup::new(control.clone(), None);
        let reader = BoundedThread::spawn("resume-fixture-reader", move || {
            crate::acp::runtime::BlockingStdoutReader::new(stdout).run(reader_mailbox.as_ref());
        })
        .unwrap();
        process_cleanup.attach_reader(reader);
        let (stdin, wire_counts) = ObservedStdin::new(stdin);
        let pump = ActivationPump::new(
            FixtureAdapter::new(
                binding_digest,
                second_epoch,
                started.session_id.clone(),
                external_session.clone(),
            )
            .unwrap(),
            CoordinatorActivationJournal::new(second_lease),
            Mapper,
            ActivationIntent {
                binding_digest,
                artifact_digest: artifact_hex.clone(),
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: second_epoch,
                mode: ActivationMode::Resume {
                    external_session_id: external_session,
                    native_root: project_root.to_string_lossy().into_owned(),
                    model: None,
                },
                curated_ceiling: capabilities.clone(),
                sage_policy: capabilities,
            },
        )
        .unwrap();
        let mut driver = ActivationProcessDriver::new(
            pump,
            stdin,
            control,
            owner,
            Arc::new(SystemDriverClock) as Arc<dyn DriverClock>,
        )
        .unwrap();
        driver.begin().unwrap();
        let mut teardown = false;
        for _ in 0..4 {
            match tokio::time::timeout(Duration::from_secs(5), driver.run_next(mailbox.as_ref()))
                .await
                .expect("resume fixture driver wake timed out")
            {
                Ok(_) => {}
                Err(ActivationDriverError::TeardownRequired(_)) => {
                    teardown = true;
                    break;
                }
                Err(error) => panic!("resume fixture driver failed unexpectedly: {error:?}"),
            }
        }
        let ready_before_cleanup: String = database
            .connection()
            .query_row(
                "SELECT state FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        if !teardown {
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                driver.run_next(mailbox.as_ref()),
            )
            .await
            .expect("resume fixture exit wake timed out");
            assert!(matches!(
                result,
                Err(ActivationDriverError::TeardownRequired(_))
            ));
        }
        let zero_handoff = driver
            .take_zero_process_handoff()
            .unwrap_or_else(|_| panic!("resume fixture did not prove zero processes"));
        process_cleanup.finish_after_zero().unwrap();
        let proof = ZeroProcessProof::from_supervisor(
            zero_handoff.owner.clone(),
            &zero_handoff.proof,
        )
        .unwrap();
        let mut lease = match case {
            ResumeCase::Success => {
                let (lease, handoff) = match zero_handoff.parts.journal.into_handoff() {
                    Ok(value) => value,
                    Err(_) => panic!("successful resume did not retain a ready lease"),
                };
                assert_eq!(handoff, CoordinatorActivationHandoff::Ready);
                lease
            }
            ResumeCase::Failure => match zero_handoff.parts.journal.into_teardown_lease() {
                Ok(lease) => lease,
                Err(_) => panic!("failed resume did not retain a teardown lease"),
            },
        };
        match case {
            ResumeCase::Success => {
                lease
                    .park_after_zero_process(
                        ParkSessionCommand {
                            project_id: project_id.clone(),
                            chat_id: chat_id.clone(),
                            session_id: started.session_id.clone(),
                            runtime_epoch: second_epoch,
                            exit_kind: RequiredText::new(
                                "exitKind",
                                "fixtureExitedAfterResume",
                            )
                            .unwrap(),
                            now_millis: now + 9,
                        },
                        proof,
                    )
                    .await
                    .unwrap();
            }
            ResumeCase::Failure => {
                lease
                    .fail_activation_after_zero_process(
                        FailActivationCommand {
                            project_id: project_id.clone(),
                            chat_id: chat_id.clone(),
                            session_id: started.session_id.clone(),
                            runtime_epoch: second_epoch,
                            binding_digest: binding_hex,
                            artifact_digest: artifact_hex,
                            terminal: SessionState::Failed,
                            exit_kind: RequiredText::new("exitKind", "resumeFailed").unwrap(),
                            now_millis: now + 9,
                        },
                        proof,
                    )
                    .await
                    .unwrap();
            }
        }
        assert_eq!(second_capacity.usage(), (0, 0));
        assert!(scheduler.active().is_none());
        assert_eq!(
            (
                wire_counts.initialize(),
                wire_counts.new_session(),
                wire_counts.resume(),
                wire_counts.prompt(),
                wire_counts.other(),
            ),
            (1, 0, 1, 0, 0),
            "resume activation must emit only one initialize and one resume"
        );
        let history_after = historical_turns(&database, &started.session_id);
        assert_eq!(history_after, history_before, "resume mutated immutable history");
        let final_row: (String, String, String) = database
            .connection()
            .query_row(
                "SELECT state,external_session_id,capabilities_json
                 FROM agent_sessions WHERE id=?1",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            final_row.2, capabilities_before,
            "recovery changed the exact persisted capability snapshot"
        );
        let observed_session = match case {
            ResumeCase::Success => ready_before_cleanup.as_str(),
            ResumeCase::Failure => final_row.0.as_str(),
        };
        let frozen = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == case.id())
            .unwrap();
        super::oracle::assert_literal(
            frozen,
            serde_json::json!({
                "priorSession": prior.1,
                "zeroProcessBeforeActivation": zero_before_activation,
                "session": observed_session,
                "resumeAttempts": wire_counts.resume(),
                "promptReplayed": wire_counts.prompt() != 0,
                "externalSessionIdPreserved": final_row.1 == EXTERNAL_SESSION,
            }),
        );

        drop(lease);
        drop(process_cleanup);
        drop(sessions);
        journal.close_when_drained();
        journal_task.await.unwrap();
        drop(journal);
        drop(database);
        temp_root.remove();
    });
}
