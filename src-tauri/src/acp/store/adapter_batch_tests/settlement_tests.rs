use super::*;
use rusqlite::params;
use sage_acp_protocol::lifecycle::ServerRequestState;

fn tool_payload() -> NormalizedPayload {
    NormalizedPayload::ToolCall {
        category: ToolCategory::Read,
        label: SafeText::new("label", "Read file").unwrap(),
        input_summary: None,
        output_summary: None,
        locations: Default::default(),
        command_activity: Default::default(),
        execution: Default::default(),
    }
}

fn running_tool_item(
    owner: &AdapterStoreOwner,
    external: &str,
    now_millis: i64,
) -> AdapterInboundEvent {
    let payload = tool_payload();
    let json = serde_json::to_string(&payload).unwrap();
    AdapterInboundEvent::Item(ItemIngressCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        external_item_id: ExternalId::new("external", external).unwrap(),
        activity_id: None,
        payload,
        payload_digest: facts::hex_digest(json.as_bytes()),
        lifecycle: ItemLifecycle::Started,
        tool_state: Some(sage_acp_protocol::lifecycle::ToolState::InProgress),
        now_millis,
    })
}

fn insert_neighbour_turn_with_open_tool(fixture: &Fixture) {
    let json = serde_json::to_string(&tool_payload()).unwrap();
    fixture
        .connection
        .execute(
            "INSERT INTO turns VALUES('turn-neighbour','project-one','chat-one',?1,?2,1,
             'intent-neighbour','running','correlated',0,4,4,NULL)",
            params![
                fixture.owner.session_id.as_str(),
                fixture.owner.runtime_epoch.get() as i64
            ],
        )
        .unwrap();
    fixture
        .connection
        .execute(
            "INSERT INTO items VALUES('item-neighbour','project-one','chat-one',?1,'turn-neighbour',
             ?2,0,'primary',NULL,'toolCall','started','inProgress','neighbour-tool',?3,?4,?5,0,4,4,NULL)",
            params![
                fixture.owner.session_id.as_str(),
                fixture.owner.runtime_epoch.get() as i64,
                json,
                json.len() as i64,
                facts::hex_digest(json.as_bytes())
            ],
        )
        .unwrap();
}

fn insert_open_tool_items(fixture: &Fixture, count: u64) {
    let json = serde_json::to_string(&tool_payload()).unwrap();
    let digest = facts::hex_digest(json.as_bytes());
    for index in 0..count {
        fixture
            .connection
            .execute(
                "INSERT INTO items VALUES(?1,'project-one','chat-one',?2,?3,?4,?5,'primary',NULL,
                 'toolCall','started','inProgress',?6,?7,?8,?9,0,4,4,NULL)",
                params![
                    format!("item-open-{index}"),
                    fixture.owner.session_id.as_str(),
                    fixture.owner.turn_id.as_str(),
                    fixture.owner.runtime_epoch.get() as i64,
                    (index + 100) as i64,
                    format!("open-tool-{index}"),
                    json,
                    json.len() as i64,
                    digest
                ],
            )
            .unwrap();
    }
}

fn permission_pair(fixture: &Fixture) -> (String, Option<String>, Option<i64>) {
    fixture
        .connection
        .query_row(
            "SELECT lifecycle,tool_state,terminal_at FROM items WHERE kind='permissionRequest'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
}

fn request_state(fixture: &Fixture) -> String {
    fixture
        .connection
        .query_row("SELECT state FROM server_requests", [], |row| row.get(0))
        .unwrap()
}

fn resolution(owner: &AdapterStoreOwner, intent: &PermissionIntent) -> AdapterInboundEvent {
    AdapterInboundEvent::PermissionResolved(RequestStateCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        request_id: intent.request_id.clone(),
        state: ServerRequestState::Cancelled,
        now_millis: 10,
    })
}

#[test]
fn an_interrupted_adapter_turn_terminal_settles_only_its_own_open_tool_item() {
    let mut fixture = fixture();
    insert_neighbour_turn_with_open_tool(&fixture);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 6,
                },
                running_tool_item(&fixture.owner, "owned-tool", 7),
            ],
        ),
    )
    .unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Interrupted,
                now_millis: 9,
            }],
        ),
    )
    .unwrap();

    let owned: (String, Option<String>, Option<i64>, i64) = fixture
        .connection
        .query_row(
            "SELECT lifecycle,tool_state,terminal_at,revision FROM items
             WHERE external_item_id='owned-tool'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        owned,
        ("cancelled".into(), Some("cancelled".into()), Some(9), 1)
    );
    let neighbour: (String, Option<String>, Option<i64>, i64) = fixture
        .connection
        .query_row(
            "SELECT lifecycle,tool_state,terminal_at,revision FROM items
             WHERE external_item_id='neighbour-tool'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        neighbour,
        ("started".into(), Some("inProgress".into()), None, 0)
    );
}

#[test]
fn the_read_path_projects_a_terminal_tool_card_after_an_interrupted_turn() {
    let mut fixture = fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 6,
                },
                running_tool_item(&fixture.owner, "owned-tool", 7),
            ],
        ),
    )
    .unwrap();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Interrupted,
                now_millis: 9,
            }],
        ),
    )
    .unwrap();

    let page = load_chat_page(
        &fixture.connection,
        fixture.owner.project_id.clone(),
        fixture.owner.chat_id.clone(),
        None,
    )
    .unwrap();
    let turn = &page.turns.as_slice()[0];
    assert_eq!(turn.state, TurnState::Interrupted);
    let tool = turn
        .items
        .as_slice()
        .iter()
        .find(|item| matches!(item.content, ItemContent::ToolCall { .. }))
        .expect("the tool card must be projected");
    assert_eq!(tool.lifecycle, ItemLifecycle::Cancelled);
    assert_eq!(
        tool.tool_state,
        Some(sage_acp_protocol::lifecycle::ToolState::Cancelled)
    );
    assert_eq!(tool.terminal_at, Some(9));
}

#[test]
fn a_harness_permission_resolution_after_the_turn_terminal_settles_the_pair() {
    let mut fixture = fixture();
    let intent = permission(&fixture.owner);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 6,
                },
                running_tool_item(&fixture.owner, "owned-tool", 7),
                AdapterInboundEvent::PermissionRequest {
                    intent: intent.clone(),
                    now_millis: 8,
                },
                AdapterInboundEvent::Turn {
                    state: TurnState::Interrupted,
                    now_millis: 9,
                },
            ],
        ),
    )
    .unwrap();
    assert_eq!(
        permission_pair(&fixture),
        ("started".into(), Some("waitingForApproval".into()), None)
    );

    let resolved = batch(&fixture.owner, vec![resolution(&fixture.owner, &intent)]);
    let first = apply_adapter_inbound_batch(&mut fixture.connection, resolved.clone()).unwrap();
    let replay = apply_adapter_inbound_batch(&mut fixture.connection, resolved).unwrap();

    assert_eq!(first.disposition, MutationDisposition::Applied);
    assert_eq!(replay.disposition, MutationDisposition::Duplicate);
    assert_eq!(request_state(&fixture), "cancelled");
    assert_eq!(
        permission_pair(&fixture),
        ("completed".into(), Some("cancelled".into()), Some(10))
    );
    let tool: (String, Option<String>) = fixture
        .connection
        .query_row(
            "SELECT lifecycle,tool_state FROM items WHERE external_item_id='owned-tool'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(tool, ("cancelled".into(), Some("cancelled".into())));
}

#[test]
fn the_turn_terminal_settles_the_maximum_admitted_open_items_beside_a_permission_item() {
    let mut fixture = fixture();
    let intent = permission(&fixture.owner);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 6,
                },
                AdapterInboundEvent::PermissionRequest {
                    intent,
                    now_millis: 7,
                },
            ],
        ),
    )
    .unwrap();
    insert_open_tool_items(&fixture, LIMITS.wire.items_per_turn);

    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Interrupted,
                now_millis: 9,
            }],
        ),
    )
    .unwrap();

    let settled: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE kind='toolCall' AND lifecycle='cancelled'
             AND tool_state='cancelled' AND terminal_at=9",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(settled, LIMITS.wire.items_per_turn as i64);
    assert_eq!(
        permission_pair(&fixture),
        ("started".into(), Some("waitingForApproval".into()), None)
    );
}

#[test]
fn adapter_interrupted_after_a_durable_user_cancel_projects_user_attribution() {
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
            client_intent_id: ClientIntentId::parse("cancel-interrupted-terminal").unwrap(),
        },
    )
    .unwrap();
    let mutation = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::Turn {
                state: TurnState::Interrupted,
                now_millis: 7,
            }],
        ),
    )
    .unwrap();

    assert_eq!(mutation.turn_cursor().state(), TurnState::Interrupted);
    let reason: (String, i64) = fixture
        .connection
        .query_row(
            "SELECT kind,created_at FROM acp_turn_terminal_reasons WHERE turn_id=?1",
            [fixture.owner.turn_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(reason, ("cancelledByUser".into(), 7));
}

#[test]
fn an_item_event_after_the_turn_terminal_rolls_back_the_whole_batch() {
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
    let before = projection(&fixture.connection);

    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(
                &fixture.owner,
                vec![
                    AdapterInboundEvent::Turn {
                        state: TurnState::Interrupted,
                        now_millis: 9,
                    },
                    running_tool_item(&fixture.owner, "late-tool", 10),
                ],
            ),
        ),
        Err(StoreError::Conflict {
            resource: "itemOwner"
        })
    ));
    let state: String = fixture
        .connection
        .query_row("SELECT state FROM turns", [], |row| row.get(0))
        .unwrap();
    let late: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE external_item_id='late-tool'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        (state, late, projection(&fixture.connection)),
        ("running".into(), 0, before)
    );
}
