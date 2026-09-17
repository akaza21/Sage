use std::time::Instant;

use crate::{
    acp::{
        contract::{ClientIntentId, ProjectGeneration, RuntimeEpoch, SageChatId, SageProjectId},
        intent::{
            ActivateAcpSessionRequest, BindingId, CancelAcpTurnRequest, CloseAcpSessionRequest,
            GetAcpChatPageRequest, Prompt, SendAcpTurnRequest, StartAcpSessionRequest,
        },
        lifecycle::TurnState,
        process::TreeCapacityPool,
        projection::{ItemContent, TextRole},
        runtime::HostState,
        scheduler::RuntimeOwner,
        service::{
            CHARACTERIZATION_BINDING_ID, CharacterizationArtifact, CharacterizationBinding,
            CharacterizationCase, CharacterizationError,
        },
    },
    commands::acp::{
        activate_acp_session_inner, cancel_acp_turn_inner, close_acp_session_inner,
        get_acp_chat_page_inner, send_acp_turn_inner, start_acp_session_inner,
    },
    persistence::Database,
    state::AppState,
};

use super::{
    external_fixture_adapters::fixture_artifact, external_fixture_support::TempRoot,
    test_support::FixtureDeadline,
};

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn characterization_commands_run_one_durable_turn_without_leaking_a_process() {
    run(CharacterizationCase::TurnSuccess, TurnState::Completed);
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn characterization_cancel_uses_the_generic_command_and_converges() {
    run(CharacterizationCase::TurnCancelled, TurnState::Cancelled);
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn characterization_cold_cycle_observes_output_then_cancels_and_converges() {
    run(CharacterizationCase::ColdCycle, TurnState::Cancelled);
}

fn run(case: CharacterizationCase, expected_terminal: TurnState) {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let deadline = FixtureDeadline::new();
        let temp_root = TempRoot::new("sage-characterization-commands");
        let project_root = temp_root.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let database = Database::open(&temp_root.path().join("sage.db")).unwrap();
        let project_id =
            SageProjectId::parse(database.create_project_from_root(&project_root).unwrap().id)
                .unwrap();
        let chat_id = SageChatId::parse(
            database
                .bootstrap_snapshot()
                .unwrap()
                .active_tab_id
                .expect("new project has a Chat tab"),
        )
        .unwrap();
        let (fixture_path, fixture_digest, fixture_bytes) = fixture_artifact();
        let artifact =
            CharacterizationArtifact::new(fixture_path, fixture_digest, fixture_bytes as u64)
                .unwrap();
        let binding = CharacterizationBinding::new(artifact, case, temp_root.path()).unwrap();
        assert_eq!(TreeCapacityPool::global().usage(), (0, 0));
        let state = AppState::try_new_characterization(database.clone(), binding).unwrap();
        assert!(
            state
                .harnesses
                .models("sage-characterization")
                .unwrap()
                .is_empty()
        );

        let initial = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        let started = start_acp_session_inner(
            &state,
            StartAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: initial.projection_revision,
                client_intent_id: ClientIntentId::parse("characterization-start").unwrap(),
                binding_id: BindingId::new("bindingId", CHARACTERIZATION_BINDING_ID).unwrap(),
                model_id: None,
            },
        )
        .await
        .unwrap();
        let session_id = started
            .projection
            .current_session_id
            .clone()
            .expect("characterization session is current");
        assert!(database.bootstrap_snapshot().is_err());
        assert_eq!(
            database
                .bootstrap_snapshot_with_catalog(&state.harnesses)
                .unwrap()
                .selected_harness_id
                .as_deref(),
            Some("sage-characterization")
        );

        let epoch = RuntimeEpoch::new(1).unwrap();
        let activated = activate_acp_session_inner(
            &state,
            ActivateAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: started.projection.projection_revision,
                client_intent_id: ClientIntentId::parse("characterization-activate").unwrap(),
                session_id: session_id.clone(),
                runtime_epoch: epoch,
            },
        )
        .await
        .unwrap();
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch: epoch,
        };
        let sent = send_acp_turn_inner(
            &state,
            SendAcpTurnRequest::Session {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: activated.projection.projection_revision,
                client_intent_id: ClientIntentId::parse("characterization-turn").unwrap(),
                session_id: session_id.clone(),
                runtime_epoch: epoch,
                prompt: Prompt::new("prompt", "read only").unwrap(),
                attached_context: Default::default(),
            },
        )
        .await
        .unwrap();
        if expected_terminal == TurnState::Cancelled {
            let cancel_intent = if case == CharacterizationCase::ColdCycle {
                loop {
                    let page = get_acp_chat_page_inner(
                        &state,
                        GetAcpChatPageRequest {
                            project_id: project_id.clone(),
                            project_generation: ProjectGeneration::ZERO,
                            chat_id: chat_id.clone(),
                            cursor: None,
                        },
                    )
                    .await
                    .unwrap();
                    let turn = page
                        .turns
                        .as_slice()
                        .iter()
                        .find(|turn| turn.session_id == session_id)
                        .expect("cold-cycle Turn is projected");
                    if turn.items.as_slice().iter().any(|item| {
                        matches!(
                            &item.content,
                            ItemContent::Text {
                                role: TextRole::Assistant,
                                ..
                            }
                        )
                    }) {
                        break (
                            page.projection_revision,
                            turn.id.clone(),
                            turn.state,
                            turn.revision,
                        );
                    }
                    assert!(
                        Instant::now() < deadline.operation_end_before_cleanup(),
                        "characterization item was not observed before cancellation"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            } else {
                let turn = sent
                    .projection
                    .turns
                    .as_slice()
                    .iter()
                    .find(|turn| turn.session_id == session_id)
                    .expect("sent characterization Turn is projected");
                (
                    sent.projection.projection_revision,
                    turn.id.clone(),
                    turn.state,
                    turn.revision,
                )
            };
            cancel_acp_turn_inner(
                &state,
                CancelAcpTurnRequest {
                    project_id: project_id.clone(),
                    project_generation: ProjectGeneration::ZERO,
                    chat_id: chat_id.clone(),
                    expected_projection_revision: cancel_intent.0,
                    client_intent_id: ClientIntentId::parse("characterization-cancel").unwrap(),
                    session_id: session_id.clone(),
                    runtime_epoch: epoch,
                    turn_id: cancel_intent.1.clone(),
                    expected_turn_state: cancel_intent.2,
                    expected_turn_revision: cancel_intent.3,
                },
            )
            .await
            .unwrap();
            if case == CharacterizationCase::ColdCycle {
                let cancel_row: (String, String) = database
                    .connection()
                    .query_row(
                        "SELECT state,delivery_state FROM turns WHERE id=?1",
                        [cancel_intent.1.as_str()],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert!(matches!(cancel_row.0.as_str(), "cancelling" | "cancelled"));
                assert_eq!(cancel_row.1, "correlated");
            }
        }

        loop {
            if state.acp.characterization_state(&owner).unwrap() == HostState::Idle {
                break;
            }
            assert!(
                Instant::now() < deadline.operation_end_before_cleanup(),
                "characterization Turn did not complete before cleanup reserve"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let completed = get_acp_chat_page_inner(
            &state,
            GetAcpChatPageRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                cursor: None,
            },
        )
        .await
        .unwrap();
        let turns = completed
            .turns
            .as_slice()
            .iter()
            .filter(|turn| turn.session_id == session_id)
            .collect::<Vec<_>>();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].state, expected_terminal);

        close_acp_session_inner(
            &state,
            CloseAcpSessionRequest {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                expected_projection_revision: completed.projection_revision,
                client_intent_id: ClientIntentId::parse("characterization-close").unwrap(),
                session_id: session_id.clone(),
                runtime_epoch: epoch,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            state.acp.characterization_state(&owner),
            Err(CharacterizationError::Intent)
        );
        let durable = database
            .connection()
            .query_row(
                "SELECT s.state,s.residency,e.state,e.exit_kind,
                        (SELECT COUNT(*) FROM turns t WHERE t.session_id=s.id
                         AND t.state IN ('queued','sending','running','waitingModel',
                                         'waitingTool','waitingApproval','waitingUser','cancelling')),
                        (SELECT COUNT(*) FROM chat_session_heads h WHERE h.session_id=s.id)
                 FROM agent_sessions s JOIN session_runtime_epochs e ON e.session_id=s.id
                 WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3 AND e.runtime_epoch=?4",
                rusqlite::params![
                    project_id.as_str(),
                    chat_id.as_str(),
                    session_id.as_str(),
                    epoch.get() as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(durable, ("closed".into(), "cold".into(), "exited".into(), "userClose".into(), 0, 0));
        state.acp.shutdown_blocking().unwrap();
        drop(state);
        drop(database);
        assert_eq!(TreeCapacityPool::global().usage(), (0, 0));
        assert!(
            !temp_root
                .path()
                .join("characterization-native-receipt-v1.json")
                .exists()
        );
        let receipt = std::fs::read_to_string(
            temp_root
                .path()
                .join("characterization-native-receipt-v2.json"),
        )
        .unwrap();
        let receipt: serde_json::Value = serde_json::from_str(&receipt).unwrap();
        assert_eq!(receipt["schemaVersion"], 2);
        assert_eq!(
            receipt["case"],
            match case {
                CharacterizationCase::TurnSuccess => "turn-success",
                CharacterizationCase::TurnCancelled => "turn-cancelled",
                CharacterizationCase::ColdCycle => "cold-cycle",
            }
        );
        assert_eq!(receipt["runtimeEpoch"], 1);
        assert!(receipt["fixtureProcess"]["pid"].as_u64().unwrap() > 0);
        assert!(
            receipt["fixtureProcess"]["creationTimeFiletime100ns"]
                .as_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert_eq!(receipt["fixtureProcess"]["executableBytes"], fixture_bytes);
        assert_eq!(
            receipt["fixtureProcess"]["executableSha256"],
            fixture_digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let clock = &receipt["monotonicClock"];
        let frequency = clock["frequencyHz"].as_u64().unwrap();
        for key in [
            "spawnStartedTick",
            "processReadyTick",
            "initializeWriteStartedTick",
            "readyCommittedTick",
            "promptWriteCompletedTick",
            "closeReceivedTick",
            "jobZeroTick",
            "closeCommittedTick",
            "walConvergedTick",
        ] {
            assert!(clock[key].as_u64().unwrap() <= 9_007_199_254_740_991);
        }
        assert_eq!(
            receipt["latencyUs"]["spawnToProcessReady"].as_u64(),
            Some(duration_us(
                clock["spawnStartedTick"].as_u64().unwrap(),
                clock["processReadyTick"].as_u64().unwrap(),
                frequency,
            ))
        );
        assert_eq!(
            receipt["latencyUs"]["initialize"].as_u64(),
            Some(duration_us(
                clock["initializeWriteStartedTick"].as_u64().unwrap(),
                clock["readyCommittedTick"].as_u64().unwrap(),
                frequency,
            ))
        );
        assert_eq!(
            receipt["latencyUs"]["nativeProcessConvergence"].as_u64(),
            Some(duration_us(
                clock["closeReceivedTick"].as_u64().unwrap(),
                clock["jobZeroTick"].as_u64().unwrap(),
                frequency,
            ))
        );
        if case == CharacterizationCase::TurnCancelled {
            assert!(clock["firstNormalizedItemAcceptedTick"].is_null());
            assert!(receipt["latencyUs"]["firstNormalizedOutput"].is_null());
        } else {
            assert_eq!(
                receipt["latencyUs"]["firstNormalizedOutput"].as_u64(),
                Some(duration_us(
                    clock["promptWriteCompletedTick"].as_u64().unwrap(),
                    clock["firstNormalizedItemAcceptedTick"].as_u64().unwrap(),
                    frequency,
                ))
            );
        }
        if case == CharacterizationCase::TurnSuccess {
            for key in [
                "cancelReceivedTick",
                "cancelWriteCompletedTick",
                "cancelTerminalCommittedTick",
            ] {
                assert!(clock[key].is_null());
            }
            assert!(receipt["latencyUs"]["cancelDispatch"].is_null());
            assert!(receipt["latencyUs"]["cancelTerminal"].is_null());
        } else {
            assert_eq!(
                receipt["latencyUs"]["cancelDispatch"].as_u64(),
                Some(duration_us(
                    clock["cancelReceivedTick"].as_u64().unwrap(),
                    clock["cancelWriteCompletedTick"].as_u64().unwrap(),
                    frequency,
                ))
            );
            assert_eq!(
                receipt["latencyUs"]["cancelTerminal"].as_u64(),
                Some(duration_us(
                    clock["cancelReceivedTick"].as_u64().unwrap(),
                    clock["cancelTerminalCommittedTick"].as_u64().unwrap(),
                    frequency,
                ))
            );
        }
        assert_eq!(
            receipt["walConvergence"]["checkpointTargetBytes"],
            16_777_216
        );
        assert!(
            receipt["walConvergence"]["walBytes"].as_u64().unwrap()
                < receipt["walConvergence"]["checkpointTargetBytes"]
                    .as_u64()
                    .unwrap()
        );
        assert!(receipt["walConvergence"]["checkpointed"].is_boolean());
        temp_root.remove();
    });
}

fn duration_us(start: u64, end: u64, frequency: u64) -> u64 {
    u64::try_from((u128::from(end - start) * 1_000_000).div_ceil(u128::from(frequency))).unwrap()
}
