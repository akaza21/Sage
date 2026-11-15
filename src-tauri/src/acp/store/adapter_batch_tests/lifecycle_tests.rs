use super::*;

#[test]
fn running_correlates_the_send_and_wait_states_preserve_delivery() {
    let mut fixture = fixture();
    let events = vec![
        AdapterInboundEvent::Turn {
            state: TurnState::Running,
            now_millis: 6,
        },
        AdapterInboundEvent::Turn {
            state: TurnState::WaitingModel,
            now_millis: 7,
        },
    ];
    let mutation =
        apply_adapter_inbound_batch(&mut fixture.connection, batch(&fixture.owner, events))
            .unwrap();
    let row: (String, String) = fixture
        .connection
        .query_row("SELECT state,delivery_state FROM turns", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(row, ("waitingModel".into(), "correlated".into()));
    assert_eq!(mutation.turn_cursor().turn_id(), &fixture.owner.turn_id);
    assert_eq!(mutation.turn_cursor().state(), TurnState::WaitingModel);
    assert_eq!(
        mutation.turn_cursor().delivery_state(),
        sage_acp_protocol::lifecycle::DeliveryState::Correlated
    );
    assert_eq!(mutation.turn_cursor().revision().get(), 3);
    assert_eq!(
        mutation.turn_cursor().projection_revision(),
        mutation.projection_revision
    );
    let duplicate = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::WaitingModel,
                now_millis: 8,
            }],
        ),
    )
    .unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
}

#[test]
fn runtime_items_reload_losslessly_and_the_identical_batch_is_a_no_op() {
    let mut fixture = fixture();
    let events = vec![
        AdapterInboundEvent::Turn {
            state: TurnState::Running,
            now_millis: 6,
        },
        terminal_item(
            &fixture.owner,
            "error-one",
            NormalizedPayload::Error {
                kind: None,
                code: SafeText::new("code", "providerError").unwrap(),
                message: SafeText::new("message", "safe message").unwrap(),
                retryable: true,
            },
            ItemLifecycle::Failed,
        ),
        terminal_item(
            &fixture.owner,
            "unsupported-one",
            NormalizedPayload::Unsupported {
                external_kind: SafeText::new("kind", "futureEvent").unwrap(),
                summary: SafeText::new("summary", "Unsupported event").unwrap(),
            },
            ItemLifecycle::Unsupported,
        ),
    ];
    let first = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, events.clone()),
    )
    .unwrap();
    let second =
        apply_adapter_inbound_batch(&mut fixture.connection, batch(&fixture.owner, events))
            .unwrap();
    assert_eq!(first.disposition, MutationDisposition::Applied);
    assert_eq!(second.disposition, MutationDisposition::Duplicate);
    assert_eq!(first.projection_revision, second.projection_revision);
    let page = load_chat_page(
        &fixture.connection,
        fixture.owner.project_id.clone(),
        fixture.owner.chat_id.clone(),
        None,
    )
    .unwrap();
    let projected = page.turns.as_slice()[0].items.as_slice();
    assert!(projected.iter().any(|item| matches!(
        item.content,
        ItemContent::Error { ref code, .. } if code.as_str() == "providerError"
    )));
    assert!(projected.iter().any(|item| matches!(
        item.content,
        ItemContent::Unsupported { ref category, .. } if category.as_str() == "futureEvent"
    )));
}

#[test]
fn usage_snapshots_replace_context_window_and_never_create_transcript_items() {
    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Running,
                now_millis: 6,
            }],
        ),
    )
    .unwrap();
    let usage = |input_tokens, output_tokens, context_window, now_millis| {
        AdapterInboundEvent::UsageSnapshot(TurnUsageSnapshotCommand {
            input_tokens,
            output_tokens,
            context_window,
            now_millis,
        })
    };

    let first = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![usage(10, 4, Some(272_000), 8)]),
    )
    .unwrap();
    fixture
        .connection
        .execute(
            "UPDATE turns SET state='completed',terminal_at=9,updated_at=9 WHERE id='turn-one'",
            [],
        )
        .unwrap();
    let replacement = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![usage(20, 5, Some(128_000), 10)]),
    )
    .unwrap();
    assert_eq!(first.disposition, MutationDisposition::Applied);
    assert_eq!(replacement.disposition, MutationDisposition::Applied);
    assert_ne!(replacement.projection_revision, first.projection_revision);

    let cleared = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![usage(30, 6, None, 11)]),
    )
    .unwrap();
    assert_eq!(cleared.disposition, MutationDisposition::Applied);
    let page = load_chat_page(
        &fixture.connection,
        fixture.owner.project_id.clone(),
        fixture.owner.chat_id.clone(),
        None,
    )
    .unwrap();
    let turn = &page.turns.as_slice()[0];
    let usage_items: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM items WHERE kind='usage'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(usage_items, 0);
    assert_eq!(turn.state, TurnState::Completed);
    assert_eq!(turn.terminal_at, Some(9));
    assert_eq!(
        turn.usage.as_ref().map(|usage| (
            usage.input_tokens,
            usage.output_tokens,
            usage.context_window
        )),
        Some((30, 6, None))
    );
    let terminal_trigger: i64 = fixture.connection.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name='deny_terminal_item_mutation'", [], |row| row.get(0),
    ).unwrap();
    assert_eq!(terminal_trigger, 1);
}

#[test]
fn first_terminal_wins_and_a_conflicting_terminal_fails_closed() {
    let mut fixture = fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Running,
                now_millis: 6,
            }],
        ),
    )
    .unwrap();
    let completed = batch(
        &fixture.owner,
        vec![AdapterInboundEvent::Turn {
            state: TurnState::Completed,
            now_millis: 7,
        }],
    );
    let first = apply_adapter_inbound_batch(&mut fixture.connection, completed.clone()).unwrap();
    let duplicate = apply_adapter_inbound_batch(&mut fixture.connection, completed).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert_eq!(duplicate.projection_revision, first.projection_revision);
    assert_eq!(duplicate.turn_cursor(), first.turn_cursor());
    assert_eq!(first.turn_cursor().state(), TurnState::Completed);
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(
                &fixture.owner,
                vec![AdapterInboundEvent::Turn {
                    state: TurnState::Failed,
                    now_millis: 8
                }]
            )
        ),
        Err(StoreError::TerminalConflict { resource: "turn" })
    ));
    let state: String = fixture
        .connection
        .query_row("SELECT state FROM turns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, "completed");
}

#[test]
fn deferred_running_after_cancel_keeps_the_interrupted_terminal() {
    let mut fixture = fixture();
    let revision: i64 = fixture
        .connection
        .query_row("SELECT revision FROM turns", [], |row| row.get(0))
        .unwrap();
    begin_cancel(
        &mut fixture.connection,
        BeginCancelCommand {
            turn: TurnMutationCommand {
                project_id: fixture.owner.project_id.clone(),
                chat_id: fixture.owner.chat_id.clone(),
                session_id: fixture.owner.session_id.clone(),
                runtime_epoch: fixture.owner.runtime_epoch,
                turn_id: fixture.owner.turn_id.clone(),
                expected_revision: EntityRevision::new(revision as u64).unwrap(),
                now_millis: 6,
            },
            client_intent_id: ClientIntentId::parse("cancel-running-race").unwrap(),
        },
    )
    .unwrap();
    let mutation = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 7,
                },
                AdapterInboundEvent::Turn {
                    state: TurnState::Interrupted,
                    now_millis: 8,
                },
            ],
        ),
    )
    .unwrap();
    assert_eq!(mutation.turn_cursor().state(), TurnState::Interrupted);
    let state: String = fixture
        .connection
        .query_row("SELECT state FROM turns", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, "interrupted");
}

#[test]
fn adapter_cancelled_after_a_durable_user_cancel_projects_user_attribution() {
    let mut fixture = fixture();
    let revision: i64 = fixture
        .connection
        .query_row("SELECT revision FROM turns", [], |row| row.get(0))
        .unwrap();
    begin_cancel(
        &mut fixture.connection,
        BeginCancelCommand {
            turn: TurnMutationCommand {
                project_id: fixture.owner.project_id.clone(),
                chat_id: fixture.owner.chat_id.clone(),
                session_id: fixture.owner.session_id.clone(),
                runtime_epoch: fixture.owner.runtime_epoch,
                turn_id: fixture.owner.turn_id.clone(),
                expected_revision: EntityRevision::new(revision as u64).unwrap(),
                now_millis: 6,
            },
            client_intent_id: ClientIntentId::parse("cancel-adapter-terminal").unwrap(),
        },
    )
    .unwrap();
    let mutation = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Cancelled,
                now_millis: 7,
            }],
        ),
    )
    .unwrap();

    assert_eq!(mutation.turn_cursor().state(), TurnState::Cancelled);
    let reason: String = fixture
        .connection
        .query_row(
            "SELECT kind FROM acp_turn_terminal_reasons WHERE turn_id=?1",
            [fixture.owner.turn_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reason, "cancelledByUser");
}

#[test]
fn adapter_cancelled_without_a_durable_user_cancel_keeps_reason_unattributed() {
    let mut fixture = fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Cancelled,
                now_millis: 6,
            }],
        ),
    )
    .unwrap();

    let count: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM acp_turn_terminal_reasons WHERE turn_id=?1 AND kind='cancelledByUser'",
            [fixture.owner.turn_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}
