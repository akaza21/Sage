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
        crate::acp::lifecycle::DeliveryState::Correlated
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
            "usage-one",
            NormalizedPayload::Usage {
                input_tokens: 10,
                output_tokens: 4,
            },
            ItemLifecycle::Completed,
        ),
        terminal_item(
            &fixture.owner,
            "error-one",
            NormalizedPayload::Error {
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
        ItemContent::Usage {
            input_tokens: Some(10),
            output_tokens: Some(4),
            ..
        }
    )));
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
