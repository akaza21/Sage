use rusqlite::{Connection, params};

use super::*;
use crate::acp::{
    contract::ProjectGeneration,
    intent::Prompt,
    lifecycle::{DeliveryState, ServerRequestState, SessionState, TurnState},
    limits::LIMITS,
};

#[path = "cancelled_permission_tests.rs"]
mod cancelled_permission_tests;
#[path = "image_ingress_tests.rs"]
mod image_ingress_tests;
#[path = "prepare_tests.rs"]
mod prepare_tests;
mod request_identity_tests;

pub(super) fn setup() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(
        "CREATE TABLE chats(id TEXT PRIMARY KEY, project_id TEXT, projection_revision INTEGER, updated_at INTEGER, title TEXT NOT NULL DEFAULT 'Chat: Fixture');
         CREATE TABLE chat_lineage(child_project_id TEXT, child_chat_id TEXT PRIMARY KEY, parent_project_id TEXT, parent_chat_id TEXT, source_turn_id TEXT, relation TEXT);
         CREATE TABLE acp_child_thread_transcripts(native_child_thread_id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, updated_at INTEGER);
         CREATE TABLE agent_sessions(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, state TEXT, residency TEXT, revision INTEGER, projection_revision INTEGER, updated_at INTEGER, terminal_at INTEGER);
         CREATE TABLE chat_session_heads(session_id TEXT PRIMARY KEY);
         CREATE TABLE session_runtime_epochs(session_id TEXT, runtime_epoch INTEGER, state TEXT, terminal_at INTEGER, exit_kind TEXT, started_at INTEGER, PRIMARY KEY(session_id,runtime_epoch));
         CREATE TABLE turns(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, session_id TEXT, runtime_epoch INTEGER, ordinal INTEGER, client_intent_id TEXT, state TEXT, delivery_state TEXT, revision INTEGER, created_at INTEGER, updated_at INTEGER, terminal_at INTEGER, UNIQUE(session_id,client_intent_id));
         CREATE TABLE items(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, session_id TEXT, turn_id TEXT, runtime_epoch INTEGER, ordinal INTEGER, origin TEXT, activity_id TEXT, kind TEXT, lifecycle TEXT, tool_state TEXT, external_item_id TEXT, payload_json TEXT, payload_bytes INTEGER, payload_digest TEXT, revision INTEGER, created_at INTEGER, updated_at INTEGER, terminal_at INTEGER);
         CREATE TABLE session_activities(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, session_id TEXT, turn_id TEXT, runtime_epoch INTEGER, lifecycle TEXT, revision INTEGER, updated_at INTEGER, terminal_at INTEGER);
         CREATE TABLE server_requests(id TEXT PRIMARY KEY, project_id TEXT, chat_id TEXT, session_id TEXT, turn_id TEXT, runtime_epoch INTEGER, external_request_id TEXT, payload_digest TEXT, normalized_payload_json TEXT, state TEXT, revision INTEGER, updated_at INTEGER, resolved_at INTEGER);
         CREATE TABLE session_facts(project_id TEXT, chat_id TEXT, session_id TEXT, sequence INTEGER, turn_id TEXT, kind TEXT, payload_json TEXT, payload_bytes INTEGER, payload_digest TEXT, projection_revision INTEGER, created_at INTEGER, PRIMARY KEY(session_id,sequence));
         CREATE TABLE acp_session_usage(session_id TEXT PRIMARY KEY, turn_count INTEGER, item_count INTEGER, item_bytes INTEGER, fact_count INTEGER, fact_bytes INTEGER, payload_bytes INTEGER, revision INTEGER);
         CREATE TABLE acp_project_usage(project_id TEXT PRIMARY KEY, turn_count INTEGER, item_count INTEGER, fact_count INTEGER, payload_bytes INTEGER, revision INTEGER);
         CREATE TABLE acp_store_usage(singleton INTEGER PRIMARY KEY, turn_count INTEGER, item_count INTEGER, item_bytes INTEGER, fact_count INTEGER, fact_bytes INTEGER, shared_payload_bytes INTEGER, revision INTEGER);
         INSERT INTO chats(id,project_id,projection_revision,updated_at,title) VALUES('chat-one','project-one',0,0,'Chat: Fixture');
         INSERT INTO agent_sessions VALUES('session-one','project-one','chat-one','ready','hot',0,0,0,NULL);
         INSERT INTO chat_session_heads VALUES('session-one');
         INSERT INTO session_runtime_epochs VALUES('session-one',1,'running',NULL,NULL,0);
         INSERT INTO acp_session_usage VALUES('session-one',0,0,0,0,0,0,0);
         INSERT INTO acp_project_usage VALUES('project-one',0,0,0,0,0);
         INSERT INTO acp_store_usage VALUES(1,0,0,0,0,0,0,0);",
    ).unwrap();
    connection
        .execute_batch(
            "CREATE UNIQUE INDEX snapshot_fixture_item_owner
         ON items(project_id,chat_id,session_id,turn_id,id,runtime_epoch);",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/010_acp_file_change_snapshots.sql"
        ))
        .unwrap();
    // The handmade sessions table lacks the owner key v21 references.
    connection
        .execute_batch(
            "CREATE UNIQUE INDEX fixture_session_owner ON agent_sessions(project_id,chat_id,id);",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/021_acp_session_scope.sql"
        ))
        .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE acp_turn_usage_snapshots(project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,input_tokens INTEGER,output_tokens INTEGER,context_window INTEGER,revision INTEGER,created_at INTEGER,updated_at INTEGER,PRIMARY KEY(session_id,runtime_epoch,turn_id));",
        )
        .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE acp_turn_terminal_reasons(project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,kind TEXT,message TEXT,remediation TEXT,created_at INTEGER,PRIMARY KEY(session_id,runtime_epoch,turn_id));",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/019_turn_workspace_changes.sql"
        ))
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/020_chat_title_provenance.sql"
        ))
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/023_chat_title_source.sql"
        ))
        .unwrap();
    connection
}

pub(super) fn command(prompt: &str) -> PrepareTurnCommand {
    PrepareTurnCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        project_generation: ProjectGeneration::new(3).unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        expected_projection_revision: ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::parse("intent-one").unwrap(),
        prompt: Prompt::new("prompt", prompt).unwrap(),
        now_millis: 10,
    }
}

fn turn_command(prepared: &PreparedTurn) -> TurnMutationCommand {
    TurnMutationCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: prepared.turn_id.clone(),
        expected_revision: prepared.turn_revision,
        now_millis: 11,
    }
}

#[test]
fn conflicting_client_intent_never_creates_a_second_turn() {
    let mut connection = setup();
    prepare_turn(&mut connection, command("one")).unwrap();
    assert!(matches!(
        prepare_turn(&mut connection, command("two")),
        Err(StoreError::Conflict {
            resource: "clientIntent"
        })
    ));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn durable_capacity_preserves_history_and_blocks_new_turns() {
    let mut connection = setup();
    connection
        .execute(
            "UPDATE acp_session_usage SET turn_count=?1",
            [LIMITS.durable.turns_per_session as i64],
        )
        .unwrap();
    assert!(matches!(
        prepare_turn(&mut connection, command("blocked")),
        Err(StoreError::Capacity {
            resource: "sessionTurns",
            ..
        })
    ));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn correlation_can_win_the_write_observer_race() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    let sending = begin_send(&mut connection, turn_command(&prepared)).unwrap();
    let running = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: sending.revision,
            now_millis: 12,
            ..turn_command(&prepared)
        },
        SendObservation::Correlated,
    )
    .unwrap();
    assert_eq!(
        (running.state, running.delivery_state),
        (TurnState::Running, DeliveryState::Correlated)
    );
}

#[test]
fn begin_cancel_commits_once_and_preserves_correlated_delivery() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("cancel")).unwrap();
    let sending = begin_send(&mut connection, turn_command(&prepared)).unwrap();
    let running = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: sending.revision,
            now_millis: 12,
            ..turn_command(&prepared)
        },
        SendObservation::Correlated,
    )
    .unwrap();
    let request = BeginCancelCommand {
        turn: TurnMutationCommand {
            expected_revision: running.revision,
            now_millis: 13,
            ..turn_command(&prepared)
        },
        client_intent_id: ClientIntentId::parse("cancel-intent").unwrap(),
    };
    let cancelling = begin_cancel(&mut connection, request.clone()).unwrap();
    assert_eq!(
        (cancelling.state, cancelling.delivery_state),
        (TurnState::Cancelling, DeliveryState::Correlated)
    );
    assert_eq!(cancelling.disposition, MutationDisposition::Applied);

    let replay = begin_cancel(&mut connection, request).unwrap();
    assert_eq!(replay.disposition, MutationDisposition::Duplicate);
    assert_eq!(replay.revision, cancelling.revision);
    let current = begin_cancel(
        &mut connection,
        BeginCancelCommand {
            turn: TurnMutationCommand {
                expected_revision: cancelling.revision,
                now_millis: 14,
                ..turn_command(&prepared)
            },
            client_intent_id: ClientIntentId::parse("cancel-intent").unwrap(),
        },
    )
    .unwrap();
    assert_eq!(current.disposition, MutationDisposition::Duplicate);
    assert_eq!(current.revision, cancelling.revision);
    let intent_facts: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='turnCancelIntent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(intent_facts, 1);
    assert!(matches!(
        begin_cancel(
            &mut connection,
            BeginCancelCommand {
                turn: TurnMutationCommand {
                    expected_revision: cancelling.revision,
                    now_millis: 15,
                    ..turn_command(&prepared)
                },
                client_intent_id: ClientIntentId::parse("cancel-other").unwrap(),
            }
        ),
        Err(StoreError::Conflict {
            resource: "cancelIntent"
        })
    ));

    let mut writing_connection = setup();
    let prepared = prepare_turn(&mut writing_connection, command("cancel-writing")).unwrap();
    let sending = begin_send(&mut writing_connection, turn_command(&prepared)).unwrap();
    let cancelling = begin_cancel(
        &mut writing_connection,
        BeginCancelCommand {
            turn: TurnMutationCommand {
                expected_revision: sending.revision,
                ..turn_command(&prepared)
            },
            client_intent_id: ClientIntentId::parse("cancel-writing").unwrap(),
        },
    )
    .unwrap();
    assert_eq!(cancelling.delivery_state, DeliveryState::Writing);
}

#[test]
fn begin_cancel_rejects_invalid_foreign_and_stale_turn_intents() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("cancel-errors")).unwrap();
    assert!(matches!(
        begin_cancel(
            &mut connection,
            BeginCancelCommand {
                turn: TurnMutationCommand {
                    expected_revision: prepared.turn_revision,
                    ..turn_command(&prepared)
                },
                client_intent_id: ClientIntentId::parse("cancel-invalid").unwrap(),
            }
        ),
        Err(StoreError::InvalidTransition {
            resource: "turnCancel"
        })
    ));
    let sending = begin_send(&mut connection, turn_command(&prepared)).unwrap();
    let running = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: sending.revision,
            ..turn_command(&prepared)
        },
        SendObservation::Correlated,
    )
    .unwrap();
    assert!(matches!(
        begin_cancel(
            &mut connection,
            BeginCancelCommand {
                turn: TurnMutationCommand {
                    expected_revision: EntityRevision::ZERO,
                    ..turn_command(&prepared)
                },
                client_intent_id: ClientIntentId::parse("cancel-stale").unwrap(),
            }
        ),
        Err(StoreError::Conflict {
            resource: "turnRevision"
        })
    ));
    assert!(matches!(
        begin_cancel(
            &mut connection,
            BeginCancelCommand {
                turn: TurnMutationCommand {
                    turn_id: SageTurnId::parse("turn-foreign").unwrap(),
                    expected_revision: running.revision,
                    ..turn_command(&prepared)
                },
                client_intent_id: ClientIntentId::parse("cancel-foreign").unwrap(),
            }
        ),
        Err(StoreError::Conflict { resource: "turn" })
    ));
}

#[test]
fn cancel_intent_fact_failure_rolls_back_state_and_projection() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("cancel-rollback")).unwrap();
    let sending = begin_send(&mut connection, turn_command(&prepared)).unwrap();
    let running = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: sending.revision,
            ..turn_command(&prepared)
        },
        SendObservation::Correlated,
    )
    .unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER fail_cancel_intent BEFORE INSERT ON session_facts
             WHEN NEW.kind='turnCancelIntent'
             BEGIN SELECT RAISE(ABORT, 'injected cancel fact failure'); END;",
        )
        .unwrap();
    assert!(matches!(
        begin_cancel(
            &mut connection,
            BeginCancelCommand {
                turn: TurnMutationCommand {
                    expected_revision: running.revision,
                    ..turn_command(&prepared)
                },
                client_intent_id: ClientIntentId::parse("cancel-rollback").unwrap(),
            }
        ),
        Err(StoreError::Database(_))
    ));
    let row: (String, i64) = connection
        .query_row("SELECT state,revision FROM turns", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(row, ("running".into(), running.revision.get() as i64));
    let projection: i64 = connection
        .query_row("SELECT projection_revision FROM chats", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(projection, running.projection_revision.get() as i64);
}

#[test]
fn possible_delivery_is_terminal_and_conflicts_cannot_overwrite_it() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    let sending = begin_send(&mut connection, turn_command(&prepared)).unwrap();
    let unknown = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: sending.revision,
            now_millis: 12,
            ..turn_command(&prepared)
        },
        SendObservation::PossibleBytes,
    )
    .unwrap();
    let duplicate = apply_turn_terminal(
        &mut connection,
        TurnMutationCommand {
            expected_revision: EntityRevision::ZERO,
            now_millis: 13,
            ..turn_command(&prepared)
        },
        TurnState::DeliveryUnknown,
        DeliveryState::PossibleBytes,
    )
    .unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert!(matches!(
        apply_turn_terminal(
            &mut connection,
            TurnMutationCommand {
                expected_revision: unknown.revision,
                now_millis: 14,
                ..turn_command(&prepared)
            },
            TurnState::Failed,
            DeliveryState::PossibleBytes
        ),
        Err(StoreError::TerminalConflict { resource: "turn" })
    ));
}

#[test]
fn multi_mutation_store_batch_rolls_back_its_complete_prefix() {
    let mut connection = setup();
    let batch = [
        StoreMutation::PrepareTurn(command("first")),
        StoreMutation::PrepareTurn(command("conflicting")),
    ];
    assert!(matches!(
        commit_batch(&mut connection, &batch),
        Err(StoreError::Conflict {
            resource: "clientIntent"
        })
    ));
    for table in ["turns", "items", "session_facts"] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table} exposed a rolled-back prefix");
    }
}

#[test]
fn mixed_projection_or_lookup_write_batches_are_rejected_before_mutation() {
    let mut connection = setup();
    let mut foreign = command("foreign");
    foreign.project_id = SageProjectId::parse("project-two").unwrap();
    let mixed_projection = [
        StoreMutation::PrepareTurn(command("first")),
        StoreMutation::PrepareTurn(foreign),
    ];
    assert!(matches!(
        commit_batch(&mut connection, &mixed_projection),
        Err(StoreError::BatchShape)
    ));
    let lookup_write = [
        StoreMutation::LookupTurn(command("first")),
        StoreMutation::PrepareTurn(command("first")),
    ];
    assert!(matches!(
        commit_batch(&mut connection, &lookup_write),
        Err(StoreError::BatchShape)
    ));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn same_chat_batch_returns_each_revision_and_the_final_authoritative_revision() {
    let mut connection = setup();
    let first = command("first");
    let mut second = command("second");
    second.client_intent_id = ClientIntentId::parse("intent-two").unwrap();
    second.expected_projection_revision = ProjectionRevision::new(1).unwrap();
    let commit = commit_batch(
        &mut connection,
        &[
            StoreMutation::PrepareTurn(first),
            StoreMutation::PrepareTurn(second),
        ],
    )
    .unwrap();
    let revisions: Vec<u64> = commit
        .results
        .iter()
        .map(|result| match result {
            StoreMutationResult::Prepared(prepared) => prepared.projection_revision.get(),
            _ => panic!("unexpected result kind"),
        })
        .collect();
    assert_eq!(revisions, [1, 2]);
    assert_eq!(commit.projection_revision.get(), 2);
}

#[test]
fn zero_byte_retry_and_process_loss_are_classified_without_prompt_replay() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    let first_send = begin_send(&mut connection, turn_command(&prepared)).unwrap();
    let zero = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: first_send.revision,
            ..turn_command(&prepared)
        },
        SendObservation::ZeroBytes,
    )
    .unwrap();
    let retry = begin_send(
        &mut connection,
        TurnMutationCommand {
            expected_revision: zero.revision,
            ..turn_command(&prepared)
        },
    )
    .unwrap();
    let zero_again = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: retry.revision,
            ..turn_command(&prepared)
        },
        SendObservation::ZeroBytes,
    )
    .unwrap();
    let failed = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: zero_again.revision,
            ..turn_command(&prepared)
        },
        SendObservation::ProcessLost,
    )
    .unwrap();
    assert_eq!(
        (failed.state, failed.delivery_state),
        (TurnState::Failed, DeliveryState::ZeroBytes)
    );
    let replay = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: zero_again.revision,
            ..turn_command(&prepared)
        },
        SendObservation::ProcessLost,
    )
    .unwrap();
    assert_eq!(replay.disposition, MutationDisposition::Duplicate);
    assert_eq!(replay.revision, failed.revision);
}

#[test]
fn process_loss_before_dispatch_cancels_the_prepared_turn_without_claiming_bytes() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    let failed = record_send_observation(
        &mut connection,
        turn_command(&prepared),
        SendObservation::ProcessLost,
    )
    .unwrap();
    assert_eq!(
        (failed.state, failed.delivery_state),
        (TurnState::Cancelled, DeliveryState::NotWritten)
    );
    let replay = record_send_observation(
        &mut connection,
        turn_command(&prepared),
        SendObservation::ProcessLost,
    )
    .unwrap();
    assert_eq!(replay.disposition, MutationDisposition::Duplicate);
    assert_eq!(replay.revision, failed.revision);
}

#[test]
fn cancelling_process_loss_uses_delivery_evidence_without_guessing() {
    fn classify(observation: Option<SendObservation>) -> (TurnState, DeliveryState) {
        let mut connection = setup();
        let prepared = prepare_turn(&mut connection, command("cancel-loss")).unwrap();
        let sending = begin_send(&mut connection, turn_command(&prepared)).unwrap();
        let before_cancel = if let Some(observation) = observation {
            record_send_observation(
                &mut connection,
                TurnMutationCommand {
                    expected_revision: sending.revision,
                    ..turn_command(&prepared)
                },
                observation,
            )
            .unwrap()
        } else {
            sending
        };
        let cancelling = begin_cancel(
            &mut connection,
            BeginCancelCommand {
                turn: TurnMutationCommand {
                    expected_revision: before_cancel.revision,
                    ..turn_command(&prepared)
                },
                client_intent_id: ClientIntentId::parse("cancel-loss-intent").unwrap(),
            },
        )
        .unwrap();
        let terminal = record_send_observation(
            &mut connection,
            TurnMutationCommand {
                expected_revision: cancelling.revision,
                ..turn_command(&prepared)
            },
            SendObservation::ProcessLost,
        )
        .unwrap();
        (terminal.state, terminal.delivery_state)
    }

    assert_eq!(
        classify(None),
        (TurnState::DeliveryUnknown, DeliveryState::PossibleBytes)
    );
    assert_eq!(
        classify(Some(SendObservation::ZeroBytes)),
        (TurnState::Cancelled, DeliveryState::ZeroBytes)
    );
    assert_eq!(
        classify(Some(SendObservation::CompleteBytes)),
        (TurnState::DeliveryUnknown, DeliveryState::CompleteBytes)
    );
    assert_eq!(
        classify(Some(SendObservation::Correlated)),
        (TurnState::Interrupted, DeliveryState::Correlated)
    );
}

#[test]
fn complete_uncorrelated_write_becomes_delivery_unknown_on_process_loss() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    let sending = begin_send(&mut connection, turn_command(&prepared)).unwrap();
    let complete = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: sending.revision,
            ..turn_command(&prepared)
        },
        SendObservation::CompleteBytes,
    )
    .unwrap();
    let unknown = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: complete.revision,
            ..turn_command(&prepared)
        },
        SendObservation::ProcessLost,
    )
    .unwrap();
    assert_eq!(
        (unknown.state, unknown.delivery_state),
        (TurnState::DeliveryUnknown, DeliveryState::CompleteBytes)
    );
    let replay = record_send_observation(
        &mut connection,
        TurnMutationCommand {
            expected_revision: complete.revision,
            ..turn_command(&prepared)
        },
        SendObservation::ProcessLost,
    )
    .unwrap();
    assert_eq!(replay.disposition, MutationDisposition::Duplicate);
    assert_eq!(replay.revision, unknown.revision);
}

#[test]
fn every_durable_terminal_family_is_idempotent_and_conflict_safe() {
    let mut connection = setup();
    let session = SessionTerminalCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        terminal: SessionState::Failed,
        now_millis: 20,
    };
    assert_eq!(
        apply_session_terminal(&mut connection, session.clone())
            .unwrap()
            .disposition,
        MutationDisposition::Applied
    );
    assert_eq!(
        apply_session_terminal(&mut connection, session.clone())
            .unwrap()
            .disposition,
        MutationDisposition::Duplicate
    );
    assert!(matches!(
        apply_session_terminal(
            &mut connection,
            SessionTerminalCommand {
                terminal: SessionState::Closed,
                ..session
            }
        ),
        Err(StoreError::TerminalConflict {
            resource: "session"
        })
    ));

    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    let request_id = SageServerRequestId::parse("request-one").unwrap();
    connection.execute(
        "INSERT INTO items VALUES('permission-item','project-one','chat-one','session-one',?1,1,1,'primary',NULL,'permissionRequest','started','waitingForApproval','external-one','{}',2,'digest-one',0,0,0,NULL)",
        [prepared.turn_id.as_str()],
    ).unwrap();
    connection.execute(
        "INSERT INTO server_requests VALUES(?1,'project-one','chat-one','session-one',?2,1,'external-one','digest-one','{}','received',0,0,NULL)",
        params![request_id.as_str(),prepared.turn_id.as_str()],
    ).unwrap();
    for (index, state) in [
        ServerRequestState::DeclinePending,
        ServerRequestState::DeclineCommitted,
        ServerRequestState::ResponseQueued,
        ServerRequestState::ResolvedDeclined,
    ]
    .into_iter()
    .enumerate()
    {
        apply_request_state(
            &mut connection,
            RequestStateCommand {
                project_id: SageProjectId::parse("project-one").unwrap(),
                chat_id: SageChatId::parse("chat-one").unwrap(),
                session_id: SageAgentSessionId::parse("session-one").unwrap(),
                turn_id: prepared.turn_id.clone(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                request_id: request_id.clone(),
                state,
                now_millis: 23 + index as i64,
            },
        )
        .unwrap();
    }
    let duplicate = apply_request_state(
        &mut connection,
        RequestStateCommand {
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            session_id: SageAgentSessionId::parse("session-one").unwrap(),
            turn_id: prepared.turn_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            request_id,
            state: ServerRequestState::ResolvedDeclined,
            now_millis: 30,
        },
    )
    .unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
}
