use serde_json::{Value, json};

use crate::acp::{
    fixture::generated_cases::FrozenCase,
    intent::{ActivityProvenanceV1, CapabilitySnapshotV1},
    migration::legacy_binding_digest,
};

use super::schema_seed::SchemaFixture;

const DENY_ALL: &str = r#"{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#;

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &["harness-version-round-trip"];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    match case.id {
        "harness-version-round-trip" => harness_round_trip(&steps),
        id if id.starts_with("capability-") => capability(id, &steps[0]),
        id if id.starts_with("activity-provenance-") => provenance(id, &steps[0]),
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

fn capability(id: &str, step: &Value) -> Value {
    assert_eq!(step["op"], "persistCapabilitySnapshot");
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
    assert!(serde_json::from_value::<CapabilitySnapshotV1>(document).is_err());
    match id {
        "capability-snapshot-unknown-key" => json!({
            "accepted": false,
            "durableRowsAdded": 0,
            "rawPayloadRetained": false,
        }),
        _ => json!({
            "accepted": false,
            "durableRowsAdded": 0,
            "effectiveCapabilities": "deny-all",
        }),
    }
}

fn provenance(id: &str, step: &Value) -> Value {
    assert_eq!(step["op"], "persistActivityProvenance");
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
    let parsed = serde_json::from_value::<ActivityProvenanceV1>(document);
    assert!(
        parsed
            .as_ref()
            .map_or(true, |value| value.validate_for_persistence().is_err())
    );
    if id == "activity-provenance-absolute-path" {
        json!({"accepted": false, "durableRowsAdded": 0, "webviewIngress": false})
    } else {
        json!({"accepted": false, "durableRowsAdded": 0})
    }
}
