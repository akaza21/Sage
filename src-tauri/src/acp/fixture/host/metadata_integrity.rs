use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::acp::{
    contract::{
        RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageItemId, SageProjectId,
        SageTurnId,
    },
    fixture::generated_cases::FrozenCase,
    intent::{ActivityProvenanceV1, CapabilitySnapshotV1, ExternalId, NormalizedPayload},
    lifecycle::ActivityLifecycle,
    migration::legacy_binding_digest,
    projection::ActivityRole,
    store::{
        ActivityIngressCommand, CompleteActivationCommand, StoreError, apply_activity_ingress,
        complete_activation, load_chat_page,
    },
};

use super::schema_seed::SchemaFixture;

const DENY_ALL: &str = r#"{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#;

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "harness-version-round-trip",
    "capability-snapshot-unknown-key",
    "capability-snapshot-missing-key",
    "capability-snapshot-wrong-type",
    "activity-provenance-absolute-path",
    "activity-provenance-missing-key",
    "activity-provenance-wrong-type",
];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    match case.id {
        "harness-version-round-trip" => harness_round_trip(&steps),
        id if id.starts_with("capability-") => capability_through_store(id, &steps[0]),
        id if id.starts_with("activity-provenance-") => provenance_through_store(id, &steps[0]),
        id => panic!("unexpected metadata-integrity case {id}"),
    }
}

fn harness_round_trip(steps: &[Value]) -> Value {
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0]["op"], "migrateSession");
    assert_eq!(steps[1]["op"], "reloadSession");
    let binding = steps[0]["bindingId"].as_str().unwrap();
    let version = steps[0]["harnessVersion"].as_str().unwrap().to_owned();
    let digest = legacy_binding_digest(binding, &Some(version.clone()));
    let fixture = SchemaFixture::project_chat();
    let connection = fixture.database.connection();
    connection
        .execute(
            "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
         harness_version,capabilities_json,state,residency,legacy,created_at,updated_at,terminal_at)
         VALUES('session-a','project-a','chat-a',0,?1,?2,?3,'{}','closed','cold',1,1,1,1)",
            rusqlite::params![binding, digest, version],
        )
        .unwrap();
    let observed: (String, String) = connection
        .query_row(
            "SELECT harness_version,binding_digest FROM agent_sessions WHERE id='session-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    json!({
        "harnessVersion": observed.0,
        "bindingDigest": observed.1,
        "bindingDigestUnchanged": observed.1 == digest,
    })
}

fn capability_through_store(id: &str, step: &Value) -> Value {
    assert_eq!(step["op"], "persistCapabilitySnapshot");
    let document = capability_document(id, step);
    let encoded = serde_json::to_string(&document).unwrap();
    let fixture = activation_fixture();
    let mut connection = fixture.database.connection();
    let before = activation_observation(&connection, &encoded);
    let result = complete_activation(
        &mut connection,
        CompleteActivationCommand {
            project_id: SageProjectId::parse("project-a").unwrap(),
            chat_id: SageChatId::parse("chat-a").unwrap(),
            session_id: SageAgentSessionId::parse("session-a").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            artifact_digest: "d".repeat(64),
            external_session_id: ExternalId::new("external", "fixture-session").unwrap(),
            capabilities_json: encoded.clone(),
            now_millis: 2,
        },
    );
    assert!(matches!(
        result,
        Err(StoreError::Corrupt("activation result"))
    ));
    let after = activation_observation(&connection, &encoded);
    assert_eq!(before, after, "rejected activation changed durable truth");
    let retained: String = connection
        .query_row(
            "SELECT capabilities_json FROM agent_sessions WHERE id='session-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let retained: CapabilitySnapshotV1 = serde_json::from_str(&retained).unwrap();
    let durable_rows_added = after.fact_rows.saturating_sub(before.fact_rows);
    match id {
        "capability-snapshot-unknown-key" => json!({
            "accepted": result.is_ok(),
            "durableRowsAdded": durable_rows_added,
            "rawPayloadRetained": after.raw_payload_rows != 0,
        }),
        _ => json!({
            "accepted": result.is_ok(),
            "durableRowsAdded": durable_rows_added,
            "effectiveCapabilities": if is_deny_all(&retained) { "deny-all" } else { "enabled" },
        }),
    }
}

fn capability_document(id: &str, step: &Value) -> Value {
    let mut document: Value = serde_json::from_str(DENY_ALL).unwrap();
    match id {
        "capability-snapshot-unknown-key" => {
            document.as_object_mut().unwrap().insert(
                step["extraKey"].as_str().unwrap().into(),
                json!({"raw": true}),
            );
        }
        "capability-snapshot-missing-key" => {
            assert_eq!(step["document"], "canonicalLegacyDenyAll");
            document
                .as_object_mut()
                .unwrap()
                .remove(step["removeKey"].as_str().unwrap());
        }
        "capability-snapshot-wrong-type" => {
            assert_eq!(step["document"], "canonicalLegacyDenyAll");
            for (key, value) in step["replace"].as_object().unwrap() {
                document
                    .as_object_mut()
                    .unwrap()
                    .insert(key.clone(), value.clone());
            }
        }
        _ => unreachable!(),
    }
    document
}

fn provenance_through_store(id: &str, step: &Value) -> Value {
    assert_eq!(step["op"], "persistActivityProvenance");
    let mut document = provenance_document(step);
    let fixture = activity_fixture();
    let mut connection = fixture.database.connection();
    let before: i64 = connection
        .query_row("SELECT COUNT(*) FROM session_activities", [], |row| {
            row.get(0)
        })
        .unwrap();
    let parsed = serde_json::from_value::<ActivityProvenanceV1>(document.take());
    let result = parsed.map_err(|_| ()).and_then(|provenance| {
        let encoded = serde_json::to_string(&provenance).unwrap();
        apply_activity_ingress(
            &mut connection,
            ActivityIngressCommand {
                project_id: SageProjectId::parse("project-a").unwrap(),
                chat_id: SageChatId::parse("chat-a").unwrap(),
                session_id: SageAgentSessionId::parse("session-a").unwrap(),
                turn_id: SageTurnId::parse("turn-a").unwrap(),
                runtime_epoch: RuntimeEpoch::new(2).unwrap(),
                external_activity_id: ExternalId::new("activity", "metadata-fixture").unwrap(),
                activity_kind: RequiredText::new("activityKind", "subagent").unwrap(),
                parent_activity_id: None,
                parent_item_id: SageItemId::parse("item-a").unwrap(),
                role: ActivityRole::Child,
                lifecycle: ActivityLifecycle::Observed,
                depth: 1,
                observed_sequence: 1,
                provenance,
                provenance_digest: digest(encoded.as_bytes()),
                now_millis: 10,
            },
        )
        .map(|_| ())
        .map_err(|_| ())
    });
    assert!(result.is_err());
    let after: i64 = connection
        .query_row("SELECT COUNT(*) FROM session_activities", [], |row| {
            row.get(0)
        })
        .unwrap();
    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project-a").unwrap(),
        SageChatId::parse("chat-a").unwrap(),
        None,
    )
    .unwrap();
    let projected_activities = page
        .turns
        .as_slice()
        .iter()
        .map(|turn| turn.activities.as_slice().len())
        .sum::<usize>();
    let common = json!({"accepted": result.is_ok(), "durableRowsAdded": after - before});
    if id == "activity-provenance-absolute-path" {
        json!({
            "accepted": common["accepted"],
            "durableRowsAdded": common["durableRowsAdded"],
            "webviewIngress": projected_activities != 0,
        })
    } else {
        common
    }
}

fn provenance_document(step: &Value) -> Value {
    let template = || {
        json!({
            "schemaVersion": 1,
            "sourceKind": "tool",
            "externalKind": "toolCall",
            "extensionSchemaId": null,
            "safeLabel": "Tool",
        })
    };
    let mut document = match step.get("document") {
        Some(value) if value.is_object() => value.clone(),
        Some(value) => {
            assert_eq!(value, "knownTool");
            template()
        }
        None => template(),
    };
    if let Some(key) = step.get("removeKey").and_then(Value::as_str) {
        document.as_object_mut().unwrap().remove(key);
    }
    if let Some(replacement) = step.get("replace").and_then(Value::as_object) {
        for (key, value) in replacement {
            document
                .as_object_mut()
                .unwrap()
                .insert(key.clone(), value.clone());
        }
    }
    document
}

#[derive(Debug, PartialEq, Eq)]
struct ActivationObservation {
    fact_rows: i64,
    raw_payload_rows: i64,
    session_state: String,
    epoch_state: String,
}

fn activation_observation(
    connection: &rusqlite::Connection,
    raw_payload: &str,
) -> ActivationObservation {
    ActivationObservation {
        fact_rows: connection
            .query_row("SELECT COUNT(*) FROM session_facts", [], |row| row.get(0))
            .unwrap(),
        raw_payload_rows: connection
            .query_row(
                "SELECT COUNT(*) FROM agent_sessions WHERE capabilities_json=?1",
                [raw_payload],
                |row| row.get(0),
            )
            .unwrap(),
        session_state: connection
            .query_row(
                "SELECT state FROM agent_sessions WHERE id='session-a'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
        epoch_state: connection
            .query_row(
                "SELECT state FROM session_runtime_epochs WHERE session_id='session-a' AND runtime_epoch=1",
                [],
                |row| row.get(0),
            )
            .unwrap(),
    }
}

fn activation_fixture() -> SchemaFixture {
    let fixture = SchemaFixture::project_chat();
    let connection = fixture.database.connection();
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,
               state,residency,legacy,revision,projection_revision,created_at,updated_at
             ) VALUES('session-a','project-a','chat-a',0,'fixture',?1,?2,
                      'starting','hot',0,0,0,1,1)",
            rusqlite::params!["a".repeat(64), DENY_ALL],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session_runtime_epochs(
               project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at
             ) VALUES('project-a','chat-a','session-a',1,'starting',?1,1)",
            ["d".repeat(64)],
        )
        .unwrap();
    drop(connection);
    fixture
}

fn activity_fixture() -> SchemaFixture {
    let fixture = SchemaFixture::project_chat();
    let connection = fixture.database.connection();
    let payload = NormalizedPayload::Text {
        text: crate::acp::contract::SafeText::new("text", "parent").unwrap(),
    };
    let encoded = serde_json::to_string(&payload).unwrap();
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,
               capabilities_json,state,residency,legacy,revision,projection_revision,
               created_at,updated_at
             ) VALUES('session-a','project-a','chat-a',0,'fixture',?1,'fixture-session',?2,
                      'ready','hot',0,0,0,1,1)",
            rusqlite::params!["a".repeat(64), DENY_ALL],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session_runtime_epochs(
               project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at
             ) VALUES('project-a','chat-a','session-a',2,'running',?1,1)",
            ["d".repeat(64)],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO turns(
               id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
               state,delivery_state,created_at,updated_at
             ) VALUES('turn-a','project-a','chat-a','session-a',2,0,'turn-a',
                      'running','correlated',1,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO items(
               id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,
               lifecycle,payload_json,payload_bytes,payload_digest,created_at,updated_at,terminal_at
             ) VALUES('item-a','project-a','chat-a','session-a','turn-a',2,0,'primary','text',
                      'completed',?1,?2,?3,1,1,1)",
            rusqlite::params![encoded, encoded.len() as i64, digest(encoded.as_bytes())],
        )
        .unwrap();
    drop(connection);
    fixture
}

fn is_deny_all(capabilities: &CapabilitySnapshotV1) -> bool {
    !capabilities.new_session
        && !capabilities.resume_session
        && !capabilities.load_session
        && !capabilities.close_session
        && !capabilities.prompt
        && !capabilities.cancel
        && !capabilities.native_session_fork
        && !capabilities.active_turn_steer
        && !capabilities.child_conversation_projection
        && !capabilities.child_transcript_read
        && !capabilities.collaboration_event_projection
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
