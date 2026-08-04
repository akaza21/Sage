use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::acp::{
    contract::{
        RequiredText, RuntimeEpoch, SafeText, SageChatId, SageItemId, SageProjectId,
        SchemaVersionV1,
    },
    fixture::generated_cases::FrozenCase,
    intent::{ActivityProvenanceV1, ActivitySourceKind, ExternalId, NormalizedPayload},
    lifecycle::{ActivityLifecycle, ItemLifecycle},
    process::TreeCapacityPool,
    projection::ActivityRole,
    runtime::{NormalizerInput, normalize},
    store::{
        ActivityIngressCommand, ItemIngressCommand, StoreError, apply_activity_ingress,
        apply_item_ingress, load_chat_page,
    },
};

use super::schema_seed::{DIGEST_A, SchemaFixture, insert_epoch, insert_turn};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "items-per-turn-512",
    "items-per-turn-513",
    "item-bytes-per-turn-8388608",
    "item-bytes-per-turn-8388609",
    "activity-groups-32",
    "activity-groups-33",
    "collaboration-receivers-32",
    "collaboration-receivers-33",
    "activity-page-nodes-128",
    "activity-page-nodes-129",
    "activity-label-bytes-512",
    "activity-label-bytes-513",
    "activity-message-bytes-16384",
    "activity-message-bytes-16385",
];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let [step] = steps.as_slice() else {
        panic!("expected one store-bound step");
    };
    match step["op"].as_str().unwrap() {
        "persistItems" => item_bound(case.id, step),
        "persistActivities" => activity_group_bound(step),
        "persistCollaboration" => receiver_bound(step),
        "readActivityPage" => activity_page_bound(step),
        "persistActivity" => activity_text_bound(step),
        operation => panic!("unexpected store-bound operation {operation}"),
    }
}

pub(in crate::acp::fixture) fn execute_child_activity(case: &FrozenCase) -> Value {
    match case.id {
        "native-activity-child-text" => child_activity_text(),
        "native-activity-depth-4" => child_activity_depth(4),
        "native-activity-depth-5" => child_activity_depth(5),
        "native-activity-foreign-parent" => child_activity_foreign_parent(),
        _ => panic!("unregistered child activity fixture {}", case.id),
    }
}

fn child_activity_text() -> Value {
    let fixture = item_fixture_for_activity();
    let process_capacity = TreeCapacityPool::isolated_ade_0006a();
    let before_processes = process_capacity.usage();
    let before_sessions: i64 = fixture
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row.get(0))
        .unwrap();
    let mut connection = fixture.database.connection();
    let activity =
        apply_activity_ingress(&mut connection, child_activity_command(1, "item-a")).unwrap();
    let payload = NormalizedPayload::Text {
        text: SafeText::new("text", "child text").unwrap(),
    };
    let encoded = serde_json::to_string(&payload).unwrap();
    apply_item_ingress(
        &mut connection,
        ItemIngressCommand {
            project_id: SageProjectId::parse("project-a").unwrap(),
            chat_id: SageChatId::parse("chat-a").unwrap(),
            session_id: crate::acp::contract::SageAgentSessionId::parse("session-a").unwrap(),
            turn_id: crate::acp::contract::SageTurnId::parse("turn-a").unwrap(),
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            external_item_id: ExternalId::new("item", "child-text-item").unwrap(),
            activity_id: Some(activity.activity_id),
            payload,
            payload_digest: digest(encoded.as_bytes()),
            lifecycle: ItemLifecycle::Completed,
            tool_state: None,
            now_millis: 20,
        },
    )
    .unwrap();
    let child_origin: String = connection
        .query_row(
            "SELECT origin FROM items WHERE external_item_id='child-text-item'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let after_sessions: i64 = connection
        .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row.get(0))
        .unwrap();
    json!({
        "itemOrigin": child_origin,
        "newSageSession": after_sessions != before_sessions,
        "newProcessTree": process_capacity.usage() != before_processes,
    })
}

fn child_activity_depth(depth: u8) -> Value {
    let fixture = item_fixture_for_activity();
    let result = apply_activity_ingress(
        &mut fixture.database.connection(),
        child_activity_command(depth, "item-a"),
    );
    if depth == 4 {
        json!({"projection": if result.is_ok() { "accepted" } else { "unsupported" }, "depth": depth})
    } else {
        json!({"projection": if result.is_err() { "unsupported" } else { "accepted" }, "capabilityChange": false})
    }
}

fn child_activity_foreign_parent() -> Value {
    let fixture = item_fixture_for_activity();
    let before_sessions: i64 = fixture
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row.get(0))
        .unwrap();
    let result = apply_activity_ingress(
        &mut fixture.database.connection(),
        child_activity_command(1, "item-old"),
    );
    let after_sessions: i64 = fixture
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row.get(0))
        .unwrap();
    json!({
        "terminal": if result.is_err() { "failed" } else { "ready" },
        "protocolViolation": result.is_err(),
        "newSageSession": after_sessions != before_sessions,
    })
}

fn item_fixture_for_activity() -> SchemaFixture {
    let fixture = SchemaFixture::graph();
    seed_usage(&fixture);
    fixture
}

fn child_activity_command(depth: u8, parent_item: &str) -> ActivityIngressCommand {
    let provenance = provenance();
    let encoded = serde_json::to_string(&provenance).unwrap();
    ActivityIngressCommand {
        project_id: SageProjectId::parse("project-a").unwrap(),
        chat_id: SageChatId::parse("chat-a").unwrap(),
        session_id: crate::acp::contract::SageAgentSessionId::parse("session-a").unwrap(),
        turn_id: crate::acp::contract::SageTurnId::parse("turn-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        external_activity_id: ExternalId::new("activity", format!("child-depth-{depth}")).unwrap(),
        activity_kind: RequiredText::new("activityKind", "subagent").unwrap(),
        parent_activity_id: None,
        parent_item_id: SageItemId::parse(parent_item).unwrap(),
        role: ActivityRole::Child,
        lifecycle: ActivityLifecycle::Observed,
        depth,
        observed_sequence: 1,
        provenance,
        provenance_digest: digest(encoded.as_bytes()),
        now_millis: 10,
    }
}

fn item_bound(id: &str, step: &Value) -> Value {
    let lengths = if let Some(count) = step.get("count").and_then(Value::as_u64) {
        let each = field_u64(step, "serializedPayloadBytesEach");
        assert_eq!(field_u64(step, "aggregateBytes"), count * each);
        vec![each; count as usize]
    } else {
        let repeated = step["repeatedItemBytes"].as_object().unwrap();
        let bytes = repeated["bytes"].as_u64().unwrap();
        let count = repeated["count"].as_u64().unwrap();
        let mut lengths = vec![bytes; count as usize];
        if let Some(additional) = step.get("additionalItemBytes").and_then(Value::as_array) {
            lengths.extend(additional.iter().map(|value| value.as_u64().unwrap()));
        }
        lengths
    };
    let fixture = item_fixture();
    let mut connection = fixture.database.connection();
    let mut rejected = None;
    for (index, bytes) in lengths.iter().copied().enumerate() {
        let payload = payload_with_serialized_bytes(bytes as usize);
        let encoded = serde_json::to_string(&payload).unwrap();
        let command = ItemIngressCommand {
            project_id: SageProjectId::parse("project-a").unwrap(),
            chat_id: SageChatId::parse("chat-a").unwrap(),
            session_id: crate::acp::contract::SageAgentSessionId::parse("session-a").unwrap(),
            turn_id: crate::acp::contract::SageTurnId::parse("turn-a").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            external_item_id: ExternalId::new("item", format!("fixture-item-{index}")).unwrap(),
            activity_id: None,
            payload,
            payload_digest: digest(encoded.as_bytes()),
            lifecycle: ItemLifecycle::Completed,
            tool_state: None,
            now_millis: index as i64 + 10,
        };
        if let Err(error) = apply_item_ingress(&mut connection, command) {
            rejected = Some(error);
            break;
        }
    }
    match id {
        "items-per-turn-512" | "item-bytes-per-turn-8388608" => {
            assert!(rejected.is_none());
            json!({"accepted": true})
        }
        "items-per-turn-513" => {
            assert!(matches!(
                rejected,
                Some(StoreError::Capacity {
                    resource: "turnItems",
                    limit: 512
                })
            ));
            json!({"accepted": false, "error": "DatabaseCapacity", "maximum": 512})
        }
        "item-bytes-per-turn-8388609" => {
            assert!(matches!(
                rejected,
                Some(StoreError::Capacity {
                    resource: "turnItemBytes",
                    limit: 8388608
                })
            ));
            json!({
                "accepted": false,
                "error": "DatabaseCapacity",
                "maximum": 8388608,
                "itemLimitBreached": false,
                "itemCount": lengths.len(),
            })
        }
        _ => unreachable!(),
    }
}

fn item_fixture() -> SchemaFixture {
    let fixture = SchemaFixture::session();
    let connection = fixture.database.connection();
    insert_epoch(&connection, 1, "running", None);
    insert_turn(&connection, "turn-a", 1, 0, "running", "correlated", None);
    connection
        .execute(
            "INSERT INTO acp_project_usage VALUES('project-a',1,1,1,0,0,0,0)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO acp_session_usage VALUES('project-a','chat-a','session-a',1,0,0,0,0,0,0)",
            [],
        )
        .unwrap();
    drop(connection);
    fixture
}

fn payload_with_serialized_bytes(bytes: usize) -> NormalizedPayload {
    let empty = NormalizedPayload::Text {
        text: SafeText::new("text", "").unwrap(),
    };
    let overhead = serde_json::to_string(&empty).unwrap().len();
    assert!(bytes >= overhead);
    let payload = NormalizedPayload::Text {
        text: SafeText::new("text", "x".repeat(bytes - overhead)).unwrap(),
    };
    assert_eq!(serde_json::to_string(&payload).unwrap().len(), bytes);
    payload
}

fn activity_group_bound(step: &Value) -> Value {
    let groups = field_u64(step, "groups");
    assert_eq!(groups, field_u64(step, "nodes"));
    let fixture = SchemaFixture::graph();
    seed_usage(&fixture);
    let mut connection = fixture.database.connection();
    let provenance = provenance();
    let encoded = serde_json::to_string(&provenance).unwrap();
    let mut rejected = None;
    for index in 0..groups {
        let command = ActivityIngressCommand {
            project_id: SageProjectId::parse("project-a").unwrap(),
            chat_id: SageChatId::parse("chat-a").unwrap(),
            session_id: crate::acp::contract::SageAgentSessionId::parse("session-a").unwrap(),
            turn_id: crate::acp::contract::SageTurnId::parse("turn-a").unwrap(),
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            external_activity_id: ExternalId::new("activity", format!("activity-{index}")).unwrap(),
            activity_kind: RequiredText::new("activityKind", "subagent").unwrap(),
            parent_activity_id: None,
            parent_item_id: SageItemId::parse("item-a").unwrap(),
            role: ActivityRole::Child,
            lifecycle: ActivityLifecycle::Completed,
            depth: 1,
            observed_sequence: index + 1,
            provenance: provenance.clone(),
            provenance_digest: digest(encoded.as_bytes()),
            now_millis: index as i64 + 10,
        };
        if let Err(error) = apply_activity_ingress(&mut connection, command) {
            rejected = Some(error);
            break;
        }
    }
    if groups == 32 {
        assert!(rejected.is_none());
        json!({"accepted": true})
    } else {
        assert!(matches!(
            rejected,
            Some(StoreError::Capacity {
                resource: "turnActivities",
                limit: 32
            })
        ));
        json!({"accepted": false, "error": "DatabaseCapacity", "maximum": 32})
    }
}

fn seed_usage(fixture: &SchemaFixture) {
    let connection = fixture.database.connection();
    connection
        .execute(
            "INSERT INTO acp_project_usage VALUES('project-a',1,1,2,2,0,0,0)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO acp_session_usage VALUES('project-a','chat-a','session-a',2,2,4,0,0,4,0)",
            [],
        )
        .unwrap();
}

fn receiver_bound(step: &Value) -> Value {
    let count = field_u64(step, "receivers");
    let result = normalize(NormalizerInput::Collaboration {
        label: "child".into(),
        message: None,
        receiver_labels: (0..count)
            .map(|index| format!("receiver-{index}"))
            .collect(),
    });
    if count == 32 {
        let NormalizedPayload::Collaboration {
            receiver_labels, ..
        } = result.unwrap()
        else {
            unreachable!()
        };
        assert_eq!(receiver_labels.as_slice().len(), 32);
        json!({"accepted": true})
    } else {
        assert!(result.is_err());
        json!({"accepted": false, "error": "ProtocolLimit", "maximum": 32})
    }
}

fn activity_text_bound(step: &Value) -> Value {
    let label = field_u64(step, "labelBytes");
    let message = field_u64(step, "messageBytes");
    let result = normalize(NormalizerInput::Collaboration {
        label: "l".repeat(label as usize),
        message: (message != 0).then(|| "m".repeat(message as usize)),
        receiver_labels: Vec::new(),
    });
    let maximum = if label > 512 { 512 } else { 16_384 };
    if label <= 512 && message <= 16_384 {
        assert!(result.is_ok());
        json!({"accepted": true})
    } else {
        assert!(result.is_err());
        json!({"accepted": false, "error": "ProtocolLimit", "maximum": maximum})
    }
}

fn activity_page_bound(step: &Value) -> Value {
    let requested = field_u64(step, "requestedNodes");
    let counts: &[usize] = if requested == 128 {
        &[32, 32, 32, 32]
    } else {
        &[1, 32, 32, 32, 32]
    };
    let database = seeded_activity_page(counts);
    let page = load_chat_page(
        &database.connection(),
        SageProjectId::parse("project-a").unwrap(),
        SageChatId::parse("chat-a").unwrap(),
        None,
    )
    .unwrap();
    let returned = page
        .turns
        .as_slice()
        .iter()
        .map(|turn| turn.activities.as_slice().len())
        .sum::<usize>();
    assert_eq!(returned, 128);
    json!({"returnedNodesMaximum": returned, "continuationRequired": page.next_cursor.is_some()})
}

fn provenance() -> ActivityProvenanceV1 {
    ActivityProvenanceV1 {
        schema_version: SchemaVersionV1,
        source_kind: ActivitySourceKind::Collaboration,
        external_kind: RequiredText::new("externalKind", "subagent").unwrap(),
        extension_schema_id: None,
        safe_label: Some(RequiredText::new("safeLabel", "Worker").unwrap()),
    }
}

fn seeded_activity_page(counts: &[usize]) -> crate::persistence::Database {
    const CAPABILITIES: &str = r#"{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#;
    let fixture = SchemaFixture::project_chat();
    let connection = fixture.database.connection();
    connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,terminal_at,created_at,updated_at) VALUES('session-a','project-a','chat-a',0,'binding',?1,?2,'closed','cold',1,1,1)", rusqlite::params![DIGEST_A, CAPABILITIES]).unwrap();
    insert_epoch(&connection, 1, "exited", Some(1));
    let payload = serde_json::to_string(&NormalizedPayload::Text {
        text: SafeText::new("text", "parent").unwrap(),
    })
    .unwrap();
    let payload_digest = digest(payload.as_bytes());
    let provenance = serde_json::to_string(&provenance()).unwrap();
    for (turn_index, count) in counts.iter().copied().enumerate() {
        let turn = format!("turn-{turn_index}");
        let item = format!("item-{turn_index}");
        let created = 100 - turn_index as i64;
        connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,created_at,updated_at,terminal_at) VALUES(?1,'project-a','chat-a','session-a',1,?2,?1,'completed','correlated',?3,?3,?3)", rusqlite::params![turn, turn_index as i64, created]).unwrap();
        connection.execute("INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,created_at,updated_at,terminal_at) VALUES(?1,'project-a','chat-a','session-a',?2,1,0,'primary','text','completed',?3,?4,?5,?6,?6,?6)", rusqlite::params![item, turn, payload, payload.len() as i64, payload_digest, created]).unwrap();
        for activity_index in 0..count {
            connection.execute("INSERT INTO session_activities(id,project_id,chat_id,session_id,turn_id,runtime_epoch,external_activity_id,activity_kind,parent_item_id,role,lifecycle,depth,observed_sequence,provenance_json,created_at,updated_at,terminal_at) VALUES(?1,'project-a','chat-a','session-a',?2,1,?1,'subagent',?3,'child','completed',1,?4,?5,?6,?6,?6)", rusqlite::params![format!("activity-{turn_index}-{activity_index}"), turn, item, activity_index as i64 + 1, provenance, created]).unwrap();
        }
    }
    drop(connection);
    fixture.database
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn field_u64(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("missing numeric field {field}"))
}
