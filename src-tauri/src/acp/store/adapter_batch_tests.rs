use super::{extended_tests::*, *};
use crate::acp::{
    contract::{
        ClientIntentId, EntityRevision, ProjectGeneration, RequiredText, SafeText, SchemaVersionV1,
    },
    intent::{
        ActivityProvenanceV1, ActivitySourceKind, ExternalId, NormalizedPayload, Prompt,
        ToolCategory,
    },
    lifecycle::{ActivityLifecycle, ItemLifecycle, TurnState},
    limits::LIMITS,
    projection::{ActivityRole, ItemContent},
    runtime::permissions::PermissionIntent,
};

mod child_thread_tests;
mod lifecycle_tests;
mod settlement_tests;
mod snapshot_reopen_tests;
mod snapshot_tests;
mod workspace_tests;

struct Fixture {
    connection: rusqlite::Connection,
    owner: AdapterStoreOwner,
}

fn fixture() -> Fixture {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "adapter-start")).unwrap();
    activate_session(
        &mut connection,
        ActivateSessionCommand {
            project_id: project(),
            chat_id: chat(),
            expected_projection_revision: ProjectionRevision::new(1).unwrap(),
            client_intent_id: ClientIntentId::parse("adapter-activate").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            artifact_digest: "d".repeat(64),
            now_millis: 2,
        },
    )
    .unwrap();
    let external_session_id = ExternalId::new("external", "adapter-session").unwrap();
    complete_activation(
        &mut connection,
        CompleteActivationCommand {
            model_context: Default::default(),
            project_id: project(),
            chat_id: chat(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            artifact_digest: "d".repeat(64),
            external_session_id: external_session_id.clone(),
            capabilities_json: capabilities(),
            now_millis: 3,
        },
    )
    .unwrap();
    let prepared = prepare_turn(
        &mut connection,
        PrepareTurnCommand {
            project_id: project(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: chat(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            expected_projection_revision: ProjectionRevision::new(3).unwrap(),
            client_intent_id: ClientIntentId::parse("adapter-turn").unwrap(),
            prompt: Prompt::new("prompt", "test").unwrap(),
            now_millis: 4,
        },
    )
    .unwrap();
    begin_send(
        &mut connection,
        TurnMutationCommand {
            project_id: project(),
            chat_id: chat(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            turn_id: prepared.turn_id.clone(),
            expected_revision: prepared.turn_revision,
            now_millis: 5,
        },
    )
    .unwrap();
    Fixture {
        connection,
        owner: AdapterStoreOwner {
            project_id: project(),
            chat_id: chat(),
            session_id: started.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            turn_id: prepared.turn_id,
            binding_digest: "a".repeat(64),
            external_session_id,
        },
    }
}

fn migrated_fixture() -> Fixture {
    let (connection, _) = crate::acp::migration::open_or_migrate(std::path::Path::new(":memory:"))
        .unwrap()
        .into_parts();
    connection.execute_batch(&format!(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-one','Project',1,1,1);
         INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-one','project-one','C:/project','c:/project',0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Chat',0,1,1);
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-one','project-one','chat-one',0,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','adapter-session','{}','active','hot',0,0,0,1,1);
         INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-one','chat-one','session-one',0,1);
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-one','chat-one','session-one',1,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',1);
         INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-one','project-one','chat-one','session-one',1,0,'intent-one','running','correlated',0,1,1);
         INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,activity_id,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES('item-one','project-one','chat-one','session-one','turn-one',1,0,'primary',NULL,'text','completed',NULL,'item-one','{{\"kind\":\"text\",\"payload\":{{\"text\":\"prompt\"}}}}',43,'b2bf66ef982a206f5552f31203f5825ec8155829efc5a4d0f63ba817a6dbe9fe',0,1,1,1);",
        capabilities(),
    )).unwrap();
    connection.execute_batch(
        "INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision)
         VALUES('project-one',1,1,1,1,0,43,0);
         INSERT INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,fact_bytes,payload_bytes,revision)
         VALUES('project-one','chat-one','session-one',1,1,43,0,0,43,0);
         UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1,item_count=1,
           item_bytes=43,shared_payload_bytes=43 WHERE singleton=1;",
    ).unwrap();
    Fixture {
        connection,
        owner: AdapterStoreOwner {
            project_id: project(),
            chat_id: chat(),
            session_id: SageAgentSessionId::parse("session-one").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            turn_id: SageTurnId::parse("turn-one").unwrap(),
            binding_digest: "a".repeat(64),
            external_session_id: ExternalId::new("external", "adapter-session").unwrap(),
        },
    }
}

#[test]
fn event_n_failure_rolls_back_the_complete_adapter_batch() {
    let mut fixture = fixture();
    let before = projection(&fixture.connection);
    let command = batch(
        &fixture.owner,
        vec![
            AdapterInboundEvent::Item(item(&fixture.owner, "item-one")),
            AdapterInboundEvent::Turn {
                state: TurnState::WaitingModel,
                now_millis: 7,
            },
        ],
    );
    assert!(matches!(
        apply_adapter_inbound_batch(&mut fixture.connection, command),
        Err(StoreError::InvalidTransition {
            resource: "adapterTurnState"
        })
    ));
    let count: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE external_item_id='item-one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((count, projection(&fixture.connection)), (0, before));
}

#[test]
fn activity_and_child_item_commit_or_rollback_as_one_adapter_batch() {
    let mut committed = fixture();
    apply_adapter_inbound_batch(
        &mut committed.connection,
        batch(
            &committed.owner,
            vec![AdapterInboundEvent::Item(item(&committed.owner, "parent"))],
        ),
    )
    .unwrap();
    apply_adapter_inbound_batch(
        &mut committed.connection,
        batch(
            &committed.owner,
            vec![activity_item(
                &committed.owner,
                "activity-one",
                "child-one",
                true,
            )],
        ),
    )
    .unwrap();
    let linked: (String, String, String) = committed
        .connection
        .query_row(
            "SELECT i.origin,i.activity_id,a.external_activity_id
             FROM items i JOIN session_activities a ON a.id=i.activity_id
             WHERE i.external_item_id='child-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(linked.0, "harnessActivity");
    assert!(!linked.1.is_empty());
    assert_eq!(linked.2, "activity-one");

    let mut rolled_back = fixture();
    apply_adapter_inbound_batch(
        &mut rolled_back.connection,
        batch(
            &rolled_back.owner,
            vec![AdapterInboundEvent::Item(item(
                &rolled_back.owner,
                "parent",
            ))],
        ),
    )
    .unwrap();
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut rolled_back.connection,
            batch(
                &rolled_back.owner,
                vec![activity_item(
                    &rolled_back.owner,
                    "activity-one",
                    "child-one",
                    false,
                )]
            )
        ),
        Err(StoreError::Corrupt("normalized item digest"))
    ));
    let retained: (i64, i64) = rolled_back
        .connection
        .query_row(
            "SELECT
               (SELECT COUNT(*) FROM session_activities),
               (SELECT COUNT(*) FROM items WHERE external_item_id='child-one')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(retained, (0, 0));
}

#[test]
fn permission_request_and_decline_commit_are_one_atomic_projection() {
    let mut fixture = fixture();
    let intent = permission(&fixture.owner);
    let mutation = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![AdapterInboundEvent::PermissionDeny {
                intent: intent.clone(),
                now_millis: 8,
            }],
        ),
    )
    .unwrap();
    let state: String = fixture
        .connection
        .query_row(
            "SELECT state FROM server_requests WHERE id=?1",
            [intent.request_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    let item: (String, String) = fixture
        .connection
        .query_row(
            "SELECT lifecycle,tool_state FROM items WHERE external_item_id=?1",
            [intent.external_request_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "declineCommitted");
    assert_eq!(item, ("started".into(), "waitingForApproval".into()));
    assert_eq!(
        mutation.projection_revision,
        projection(&fixture.connection)
    );
}

#[test]
fn injected_decline_failure_rolls_back_the_request_item_and_received_facts() {
    let mut fixture = fixture();
    let before = projection(&fixture.connection);
    let facts_before: i64 = fixture
        .connection
        .query_row("SELECT COUNT(*) FROM session_facts", [], |row| row.get(0))
        .unwrap();
    fixture
        .connection
        .execute_batch(
            "CREATE TRIGGER inject_decline_failure
             BEFORE UPDATE OF state ON server_requests
             WHEN NEW.state='declineCommitted'
             BEGIN SELECT RAISE(ABORT, 'injected decline failure'); END;",
        )
        .unwrap();
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(
                &fixture.owner,
                vec![AdapterInboundEvent::PermissionDeny {
                    intent: permission(&fixture.owner),
                    now_millis: 8,
                }]
            )
        ),
        Err(StoreError::Database(_))
    ));
    for table in ["server_requests", "session_facts"] {
        let count: i64 = fixture
            .connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        let expected = if table == "session_facts" {
            facts_before
        } else {
            0
        };
        assert_eq!(
            count, expected,
            "{table} exposed a partial permission prefix"
        );
    }
    let item_count: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE kind='permissionRequest'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!((item_count, projection(&fixture.connection)), (0, before));
}

#[test]
fn limit_plus_one_and_foreign_ownership_fail_before_mutation() {
    let mut fixture = fixture();
    let before = projection(&fixture.connection);
    let events = (0..=LIMITS.queues.journal_facts_per_transaction)
        .map(|_| AdapterInboundEvent::Turn {
            state: TurnState::Running,
            now_millis: 6,
        })
        .collect();
    assert!(matches!(
        AdapterInboundBatchCommand::new(fixture.owner.clone(), 1024, 4096, events),
        Err(StoreError::BatchShape)
    ));
    let mut foreign = fixture.owner.clone();
    foreign.external_session_id = ExternalId::new("external", "foreign").unwrap();
    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(
                &foreign,
                vec![AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 6
                }]
            )
        ),
        Err(StoreError::Conflict {
            resource: "adapterBatchOwner"
        })
    ));
    assert_eq!(projection(&fixture.connection), before);
}

#[test]
fn durable_turn_item_limit_accepts_exact_and_rejects_plus_one_atomically() {
    let mut fixture = fixture();
    let additional = LIMITS.wire.items_per_turn - 1;
    let mut next = 0_u64;
    while next < additional {
        let take = (additional - next).min(LIMITS.queues.journal_facts_per_transaction);
        let events = (next..next + take)
            .map(|index| AdapterInboundEvent::Item(item(&fixture.owner, &format!("item-{index}"))))
            .collect();
        apply_adapter_inbound_batch(&mut fixture.connection, batch(&fixture.owner, events))
            .unwrap();
        next += take;
    }

    let exact: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE turn_id=?1",
            [fixture.owner.turn_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(exact, LIMITS.wire.items_per_turn as i64);

    assert!(matches!(
        apply_adapter_inbound_batch(
            &mut fixture.connection,
            batch(
                &fixture.owner,
                vec![AdapterInboundEvent::Item(item(&fixture.owner, "item-over"))]
            )
        ),
        Err(StoreError::Capacity {
            resource: "turnItems",
            limit
        }) if limit == LIMITS.wire.items_per_turn
    ));
    let retained: i64 = fixture
        .connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE turn_id=?1",
            [fixture.owner.turn_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained, exact);
}

#[test]
fn durable_turn_item_bytes_accept_exact_and_reject_the_next_item_atomically() {
    let mut fixture = fixture();
    let existing: i64 = fixture
        .connection
        .query_row(
            "SELECT COALESCE(SUM(payload_bytes),0) FROM items WHERE turn_id=?1",
            [fixture.owner.turn_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    let maximum = LIMITS.wire.normalized_item_bytes as usize;
    let mut remaining = LIMITS.wire.item_bytes_per_turn as usize - existing as usize;
    let minimum = serialized_text_payload_bytes(0);
    let mut sizes = Vec::new();
    while remaining > maximum {
        sizes.push(maximum);
        remaining -= maximum;
    }
    if remaining < minimum {
        let previous = sizes.pop().expect("the aggregate limit spans many items");
        remaining += previous;
        sizes.push(remaining - minimum);
        sizes.push(minimum);
    } else {
        sizes.push(remaining);
    }

    for (index, bytes) in sizes.into_iter().enumerate() {
        apply_item_ingress(
            &mut fixture.connection,
            sized_item(&fixture.owner, &format!("bytes-{index}"), bytes),
        )
        .unwrap();
    }
    let exact: i64 = fixture
        .connection
        .query_row(
            "SELECT COALESCE(SUM(payload_bytes),0) FROM items WHERE turn_id=?1",
            [fixture.owner.turn_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(exact, LIMITS.wire.item_bytes_per_turn as i64);

    assert!(matches!(
        apply_item_ingress(
            &mut fixture.connection,
            sized_item(&fixture.owner, "bytes-over", minimum)
        ),
        Err(StoreError::Capacity {
            resource: "turnItemBytes",
            limit
        }) if limit == LIMITS.wire.item_bytes_per_turn
    ));
    let retained: i64 = fixture
        .connection
        .query_row(
            "SELECT COALESCE(SUM(payload_bytes),0) FROM items WHERE turn_id=?1",
            [fixture.owner.turn_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retained, exact);
}

fn batch(
    owner: &AdapterStoreOwner,
    events: Vec<AdapterInboundEvent>,
) -> AdapterInboundBatchCommand {
    AdapterInboundBatchCommand::new(owner.clone(), 1024, 4096, events).unwrap()
}

fn item(owner: &AdapterStoreOwner, external: &str) -> ItemIngressCommand {
    let payload = NormalizedPayload::Text {
        text: SafeText::new("text", "adapter text").unwrap(),
    };
    let json = serde_json::to_string(&payload).unwrap();
    ItemIngressCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        external_item_id: ExternalId::new("external", external).unwrap(),
        activity_id: None,
        payload,
        payload_digest: facts::hex_digest(json.as_bytes()),
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        now_millis: 6,
    }
}

fn activity_item(
    owner: &AdapterStoreOwner,
    external_activity: &str,
    external_item: &str,
    valid_item_digest: bool,
) -> AdapterInboundEvent {
    let provenance = ActivityProvenanceV1 {
        schema_version: SchemaVersionV1,
        source_kind: ActivitySourceKind::Collaboration,
        external_kind: RequiredText::new("kind", "subagent").unwrap(),
        extension_schema_id: None,
        safe_label: Some(RequiredText::new("label", "Fixture child").unwrap()),
    };
    let provenance_json = serde_json::to_string(&provenance).unwrap();
    let mut child = item(owner, external_item);
    if !valid_item_digest {
        child.payload_digest = "0".repeat(64);
    }
    AdapterInboundEvent::ActivityItem(Box::new(super::AdapterActivityItemCommand {
        external_activity_id: ExternalId::new("activity", external_activity).unwrap(),
        activity_kind: RequiredText::new("activityKind", "subagent").unwrap(),
        parent_external_activity_id: None,
        parent_external_item_id: ExternalId::new("parentItem", "parent").unwrap(),
        role: ActivityRole::Child,
        activity_lifecycle: ActivityLifecycle::Observed,
        depth: 1,
        observed_sequence: 1,
        provenance,
        provenance_digest: facts::hex_digest(provenance_json.as_bytes()),
        item: child,
    }))
}

fn serialized_text_payload_bytes(text_bytes: usize) -> usize {
    serde_json::to_string(&NormalizedPayload::Text {
        text: SafeText::new("text", "x".repeat(text_bytes)).unwrap(),
    })
    .unwrap()
    .len()
}

fn sized_item(owner: &AdapterStoreOwner, external: &str, bytes: usize) -> ItemIngressCommand {
    let overhead = serialized_text_payload_bytes(0);
    assert!(bytes >= overhead);
    let payload = NormalizedPayload::Text {
        text: SafeText::new("text", "x".repeat(bytes - overhead)).unwrap(),
    };
    let json = serde_json::to_string(&payload).unwrap();
    assert_eq!(json.len(), bytes);
    ItemIngressCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        external_item_id: ExternalId::new("external", external).unwrap(),
        activity_id: None,
        payload,
        payload_digest: facts::hex_digest(json.as_bytes()),
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        now_millis: 6,
    }
}

fn terminal_item(
    owner: &AdapterStoreOwner,
    external: &str,
    payload: NormalizedPayload,
    lifecycle: ItemLifecycle,
) -> AdapterInboundEvent {
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
        lifecycle,
        tool_state: None,
        now_millis: 7,
    })
}

fn permission(owner: &AdapterStoreOwner) -> PermissionIntent {
    let payload = NormalizedPayload::PermissionRequest {
        category: ToolCategory::Execute,
        summary: SafeText::new("summary", "Run command?").unwrap(),
        offered_option_kinds: None,
        reason: None,
    };
    let json = serde_json::to_string(&payload).unwrap();
    PermissionIntent {
        request_id: SageServerRequestId::parse("request-adapter").unwrap(),
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        turn_id: owner.turn_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        binding_digest: owner.binding_digest.clone(),
        external_request_id: ExternalId::new("external", "permission-adapter").unwrap(),
        kind: RequiredText::new("kind", "permission").unwrap(),
        payload_digest: "f".repeat(64),
        normalized_payload_digest: facts::hex_digest(json.as_bytes()),
        normalized_payload_json: json,
    }
}

fn projection(connection: &rusqlite::Connection) -> ProjectionRevision {
    let revision: i64 = connection
        .query_row(
            "SELECT projection_revision FROM chats WHERE id='chat-one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    ProjectionRevision::new(revision as u64).unwrap()
}

fn project() -> SageProjectId {
    SageProjectId::parse("project-one").unwrap()
}

fn chat() -> SageChatId {
    SageChatId::parse("chat-one").unwrap()
}
