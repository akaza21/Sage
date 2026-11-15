use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{
        AdapterEvent, AdapterModel, AdapterRuntimeError, HarnessAdapter, ResourceMapper,
    },
    fixture::adapter::FixtureAdapter,
    semantic_ingress::{
        SemanticIngressError, decode_capability_token, validate_metadata, validate_models,
    },
    store::{CompleteActivationCommand, complete_activation},
    transport::{Frame, IncrementalFrameDecoder, validate_structure},
};
use sage_acp_protocol::{
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    intent::{
        CapabilitySnapshotV1, ExternalId, ModelId, SafeResourceRef, UnavailableResourceReason,
    },
};
use sage_core::limits::LIMITS;

use super::super::{generated_cases::FrozenCase, wire};

pub(crate) const CASE_IDS: &[&str] = &[
    "capability-token-bytes-65536",
    "capability-token-bytes-65537",
    "external-id-bytes-512",
    "external-id-bytes-513",
    "metadata-entries-64",
    "metadata-entries-65",
    "metadata-bytes-65536",
    "metadata-bytes-65537",
    "models-entries-128",
    "models-entries-129",
    "models-bytes-131072",
    "models-bytes-131073",
];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let (stdout, _) = wire::capture_steps(&steps).unwrap();
    let frame = decode_one(case.id, &stdout);
    match case.id {
        "capability-token-bytes-65536" | "capability-token-bytes-65537" => {
            observe_capability(frame)
        }
        "external-id-bytes-512" | "external-id-bytes-513" => observe_external_id(frame),
        "metadata-entries-64"
        | "metadata-entries-65"
        | "metadata-bytes-65536"
        | "metadata-bytes-65537" => observe_metadata(frame),
        "models-entries-128"
        | "models-entries-129"
        | "models-bytes-131072"
        | "models-bytes-131073" => observe_models(frame),
        _ => panic!("unregistered semantic fixture {}", case.id),
    }
}

fn observe_capability(frame: Frame) -> Value {
    let database = activation_database();
    let before: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM session_facts", [], |row| row.get(0))
        .unwrap();
    let mut adapter = fixture_adapter();
    let result = adapter.ingest(&frame, &Mapper);
    debug_assert_eq!(
        result.is_ok(),
        decode_capability_token(frame.as_bytes()).is_ok()
    );
    if let Ok(batch) = &result
        && let [AdapterEvent::Initialized { capabilities }] = batch.events()
    {
        complete_activation(
            &mut database.connection(),
            CompleteActivationCommand {
                project_id: SageProjectId::parse("project-a").unwrap(),
                chat_id: SageChatId::parse("chat-a").unwrap(),
                session_id: SageAgentSessionId::parse("session-a").unwrap(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                binding_digest: "a".repeat(64),
                artifact_digest: "d".repeat(64),
                external_session_id: ExternalId::new("external", "fixture-session").unwrap(),
                capabilities_json: serde_json::to_string(capabilities).unwrap(),
                now_millis: 2,
            },
        )
        .unwrap();
    }
    let after: i64 = database
        .connection()
        .query_row("SELECT COUNT(*) FROM session_facts", [], |row| row.get(0))
        .unwrap();
    match result {
        Ok(batch) => {
            let [AdapterEvent::Initialized { capabilities }] = batch.events() else {
                panic!("capability fixture did not produce initialization");
            };
            json!({
                "accepted": true,
                "normalizedDocument": if canonical_legacy_deny_all(capabilities) {
                    "canonicalLegacyDenyAll"
                } else {
                    "noncanonical"
                },
            })
        }
        Err(AdapterRuntimeError::BatchBytes) => json!({
            "accepted": false,
            "error": "ProtocolLimit",
            "durableRowsAdded": after - before,
        }),
        Err(_) => json!({
            "accepted": false,
            "error": "ProtocolInvalid",
            "durableRowsAdded": after - before,
        }),
    }
}

fn observe_external_id(frame: Frame) -> Value {
    let value = envelope_value(&frame);
    match ExternalId::new("fixtureExternalId", value) {
        Ok(_) => json!({"accepted": true}),
        Err(_) => json!({"accepted": false, "error": "ProtocolLimit"}),
    }
}

fn observe_metadata(frame: Frame) -> Value {
    let envelope: Value = serde_json::from_slice(frame.as_bytes()).unwrap();
    let metadata = envelope["params"]["_meta"].as_object().unwrap();
    let mut adapter = fixture_adapter();
    let result = adapter.ingest(&frame, &Mapper);
    debug_assert_eq!(result.is_ok(), validate_metadata(metadata).is_ok());
    match result {
        Ok(batch) if matches!(batch.events(), [AdapterEvent::Acknowledged]) => {
            json!({"accepted": true})
        }
        Err(AdapterRuntimeError::BatchBytes)
            if metadata.len() > LIMITS.wire.metadata_entries as usize =>
        {
            json!({"accepted": false, "error": "ProtocolLimit"})
        }
        Err(AdapterRuntimeError::BatchBytes) => json!({
            "accepted": false,
            "error": "ProtocolLimit",
            "entryLimitBreached": false,
        }),
        _ => {
            json!({"accepted": false, "error": "ProtocolInvalid"})
        }
    }
}

fn observe_models(frame: Frame) -> Value {
    let envelope: Value = serde_json::from_slice(frame.as_bytes()).unwrap();
    let raw = envelope["params"]["models"].as_array().unwrap();
    let models = raw
        .iter()
        .map(|value| {
            Ok(AdapterModel {
                id: ModelId::new("modelId", value["id"].as_str().unwrap().to_owned())?,
                name: RequiredText::new("modelName", value["name"].as_str().unwrap().to_owned())?,
                is_default: false,
            })
        })
        .collect::<Result<Vec<_>, sage_acp_protocol::contract::ContractError>>();
    let accepted = models
        .as_deref()
        .map_err(|_| SemanticIngressError::Invalid)
        .and_then(validate_models);
    match accepted {
        Ok(()) => json!({"accepted": true}),
        Err(SemanticIngressError::Limit)
            if raw.len() > LIMITS.wire.models_or_config_entries as usize =>
        {
            if frame.len() > LIMITS.wire.models_or_config_bytes as usize {
                json!({
                    "accepted": false,
                    "error": "ProtocolLimit",
                    "entryCountLimitAlsoBreached": true,
                })
            } else {
                json!({"accepted": false, "error": "ProtocolLimit"})
            }
        }
        Err(SemanticIngressError::Limit) => json!({"accepted": false, "error": "ProtocolLimit"}),
        Err(SemanticIngressError::Invalid) => {
            json!({"accepted": false, "error": "ProtocolInvalid"})
        }
    }
}

fn decode_one(case_id: &str, bytes: &[u8]) -> Frame {
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frame = None;
    for chunk in bytes.chunks(8 * 1024) {
        if let Some(value) = decoder.push_chunk(chunk).unwrap() {
            assert!(frame.replace(value).is_none());
        }
        while decoder.has_pending_input() {
            if let Some(value) = decoder.drain_pending().unwrap() {
                assert!(frame.replace(value).is_none());
            }
        }
    }
    if let Some(value) = decoder.finish().unwrap() {
        assert!(frame.replace(value).is_none());
    }
    let frame = frame.unwrap();
    validate_structure(&frame)
        .unwrap_or_else(|error| panic!("semantic fixture {case_id} was invalid: {error:?}"));
    frame
}

fn envelope_value(frame: &Frame) -> String {
    let envelope: Value = serde_json::from_slice(frame.as_bytes()).unwrap();
    envelope["params"]["value"].as_str().unwrap().to_owned()
}

fn canonical_legacy_deny_all(value: &CapabilitySnapshotV1) -> bool {
    value.schema_version == sage_acp_protocol::contract::SchemaVersionV1
        && value.protocol_version.as_str() == "legacy"
        && !value.new_session
        && !value.resume_session
        && !value.load_session
        && !value.close_session
        && !value.prompt
        && !value.cancel
        && !value.native_session_fork
        && !value.active_turn_steer
        && !value.child_conversation_projection
        && !value.child_transcript_read
        && !value.collaboration_event_projection
        && value.multi_root_extension_schema_id.is_none()
        && value.model_count == 0
        && value.config_count == 0
}

fn fixture_adapter() -> FixtureAdapter {
    FixtureAdapter::new(
        [7; 32],
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("fixture-session").unwrap(),
        ExternalId::new("externalSession", "fixture-external-session").unwrap(),
    )
    .unwrap()
}

fn activation_database() -> crate::persistence::Database {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-a','project-a',1,1,1)",[]).unwrap();
    connection.execute("INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-a','project-a','C:/project','c:/project',0)",[]).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat-a','project-a',0,'chat-a',1,1)",[]).unwrap();
    connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-a','project-a','chat-a',0,'fixture',?1,?2,'starting','hot',0,0,0,1,1)",rusqlite::params!["a".repeat(64), pending_capabilities()]).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-a','chat-a','session-a',1,'starting',?1,1)",["d".repeat(64)]).unwrap();
    connection
        .execute(
            "INSERT INTO acp_project_usage VALUES('project-a',1,1,0,0,0,0,0)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO acp_session_usage VALUES('project-a','chat-a','session-a',0,0,0,0,0,0,0)",
            [],
        )
        .unwrap();
    drop(connection);
    database
}

fn pending_capabilities() -> &'static str {
    r#"{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#
}

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}
