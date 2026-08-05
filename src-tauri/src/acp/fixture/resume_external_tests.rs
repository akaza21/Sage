use std::{fs, sync::Arc, time::Duration};

use uuid::Uuid;

use crate::{
    acp::{
        contract::{
            ClientIntentId, RequiredText, RuntimeEpoch, SafeText, SageChatId, SageProjectId,
        },
        fixture::adapter::FixtureAdapter,
        intent::ExternalId,
        lifecycle::SessionState,
        process::{PlatformSupervisor, TreeCapacityPool, TreeExit},
        runtime::{
            ActivationDriverError, ActivationIntent, ActivationMode, ActivationProcessDriver,
            ActivationPump, CoordinatorActivationHandoff, CoordinatorActivationJournal,
            DriverClock, DriverMailbox, JournalHandle, ProjectionPublisher, SessionCoordinator,
            SystemDriverClock, ZeroProcessProof,
        },
        scheduler::{RuntimeOwner, Scheduler},
        store::{
            ActivateSessionCommand, CompleteActivationCommand, DatabaseAcpStore,
            FailActivationCommand, ParkSessionCommand, StartSessionCommand, load_chat_page,
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

const EXTERNAL_SESSION: &str = "fixture-persisted-session-1";

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn resume_cases_run_from_cold_through_supervisor_driver_and_sqlite() {
    for (case_id, succeeds) in [
        ("resume-success-once", true),
        ("resume-failure-no-second-attempt", false),
    ] {
        run_resume_case(case_id, succeeds);
    }
}

fn run_resume_case(case_id: &'static str, succeeds: bool) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (fixture_path, artifact_digest, fixture_byte_len) = fixture_artifact();
        let artifact_hex = hex(&artifact_digest);
        let binding_digest = [7_u8; 32];
        let binding_hex = hex(&binding_digest);
        let root = std::env::temp_dir().join(format!("sage-resume-e2e-{}", Uuid::new_v4()));
        let project_root = root.join("project");
        fs::create_dir_all(&project_root).unwrap();
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
                binding_digest: binding_hex.clone(),
                harness_version: Some(SafeText::new("harnessVersion", "fixture-1").unwrap()),
                model_id: None,
                now_millis: now,
            })
            .await
            .unwrap();
        let external_session = ExternalId::new("externalSessionId", EXTERNAL_SESSION).unwrap();
        let capabilities = FixtureAdapter::activation_capabilities();

        let first_epoch = RuntimeEpoch::new(1).unwrap();
        let first_owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: started.session_id.clone(),
            runtime_epoch: first_epoch,
        };
        let first_lease = sessions
            .activate_session(activation_command(
                &first_owner,
                started.projection_revision,
                "resume-fixture-activate-1",
                &artifact_hex,
                now + 1,
            ))
            .await
            .unwrap();
        let mut first_journal = CoordinatorActivationJournal::new(first_lease);
        crate::acp::runtime::ActivationJournal::commit_ready(
            &mut first_journal,
            CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: started.session_id.clone(),
                runtime_epoch: first_epoch,
                binding_digest: binding_hex.clone(),
                artifact_digest: artifact_hex.clone(),
                external_session_id: external_session.clone(),
                capabilities_json: serde_json::to_string(&capabilities).unwrap(),
                now_millis: now + 2,
            },
        )
        .await
        .unwrap();
        let (mut first_lease, handoff) = match first_journal.into_handoff() {
            Ok(value) => value,
            Err(_) => panic!("first activation handoff was not ready"),
        };
        assert_eq!(handoff, CoordinatorActivationHandoff::Ready);

        let first_capacity = TreeCapacityPool::isolated_ade_0006a();
        let first_spec = launch_spec(
            case_id,
            first_epoch,
            fixture_path.clone(),
            artifact_digest,
            fixture_byte_len,
        );
        let first_tree = PlatformSupervisor::launch(
            &first_spec,
            first_capacity
                .try_acquire(first_spec.limits.descendants)
                .unwrap(),
        )
        .unwrap();
        let (first_stdin, first_stdout, first_control) = first_tree.into_parts();
        first_control.request_termination(TreeExit::Cancelled).unwrap();
        let (_, first_zero) = first_control.wait_zero().unwrap();
        drop((first_stdin, first_stdout, first_control));
        let first_proof = ZeroProcessProof::from_supervisor(first_owner, &first_zero).unwrap();
        let parked = first_lease
            .park_after_zero_process(
                ParkSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: started.session_id.clone(),
                    runtime_epoch: first_epoch,
                    exit_kind: RequiredText::new("exitKind", "idleAfterTurn").unwrap(),
                    now_millis: now + 3,
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
                now + 4,
            ))
            .await
            .unwrap();
        let second_spec = launch_spec(
            case_id,
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
        let reader = std::thread::spawn(move || {
            crate::acp::runtime::BlockingStdoutReader::new(stdout)
                .run(reader_mailbox.as_ref());
        });
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
                    external_session_id: external_session.clone(),
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
        let mut written_frames = driver.begin().unwrap().written_frames;
        let mut teardown = false;
        for _ in 0..4 {
            match tokio::time::timeout(Duration::from_secs(5), driver.run_next(mailbox.as_ref()))
                .await
                .expect("resume fixture driver wake timed out")
            {
                Ok(progress) => written_frames += progress.written_frames,
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
            if !matches!(result, Err(ActivationDriverError::TeardownRequired(_))) {
                panic!("resume fixture did not converge to teardown: {result:?}");
            }
        }
        let zero_handoff = driver
            .take_zero_process_handoff()
            .unwrap_or_else(|_| panic!("resume fixture did not prove zero processes"));
        let proof = ZeroProcessProof::from_supervisor(
            zero_handoff.owner.clone(),
            &zero_handoff.proof,
        )
        .unwrap();
        let mut lease = if succeeds {
            let (lease, handoff) = match zero_handoff.parts.journal.into_handoff() {
                Ok(value) => value,
                Err(_) => panic!("successful resume did not retain a ready lease"),
            };
            assert_eq!(handoff, CoordinatorActivationHandoff::Ready);
            lease
        } else {
            match zero_handoff.parts.journal.into_teardown_lease() {
                Ok(lease) => lease,
                Err(_) => panic!("failed resume did not retain a teardown lease"),
            }
        };
        if succeeds {
            lease
                .park_after_zero_process(
                    ParkSessionCommand {
                        project_id: project_id.clone(),
                        chat_id: chat_id.clone(),
                        session_id: started.session_id.clone(),
                        runtime_epoch: second_epoch,
                        exit_kind: RequiredText::new("exitKind", "fixtureExitedAfterResume").unwrap(),
                        now_millis: now + 6,
                    },
                    proof,
                )
                .await
                .unwrap();
        } else {
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
                        now_millis: now + 6,
                    },
                    proof,
                )
                .await
                .unwrap();
        }
        reader.join().unwrap();
        assert_eq!(second_capacity.usage(), (0, 0));
        assert!(scheduler.active().is_none());
        let final_row: (String, String, i64) = database
            .connection()
            .query_row(
                "SELECT state,external_session_id,
                        (SELECT COUNT(*) FROM turns WHERE session_id=s.id)
                 FROM agent_sessions s WHERE id=?1",
                [started.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        let observed_session = if succeeds {
            ready_before_cleanup.as_str()
        } else {
            final_row.0.as_str()
        };
        let case = FROZEN_CASES
            .iter()
            .find(|candidate| candidate.id == case_id)
            .unwrap();
        super::oracle::assert_literal(
            case,
            serde_json::json!({
                "priorSession": prior.1,
                "zeroProcessBeforeActivation": zero_before_activation,
                "session": observed_session,
                "resumeAttempts": written_frames.checked_sub(1).unwrap(),
                "promptReplayed": final_row.2 != 0,
                "externalSessionIdPreserved": final_row.1 == EXTERNAL_SESSION,
            }),
        );

        drop(lease);
        drop(sessions);
        journal.close_when_drained();
        journal_task.await.unwrap();
        drop(journal);
        drop(database);
        fs::remove_dir_all(root).unwrap();
    });
}

fn activation_command(
    owner: &RuntimeOwner,
    expected_projection_revision: crate::acp::contract::ProjectionRevision,
    client_intent_id: &str,
    artifact_digest: &str,
    now_millis: i64,
) -> ActivateSessionCommand {
    ActivateSessionCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        expected_projection_revision,
        client_intent_id: ClientIntentId::parse(client_intent_id).unwrap(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        artifact_digest: artifact_digest.into(),
        now_millis,
    }
}
