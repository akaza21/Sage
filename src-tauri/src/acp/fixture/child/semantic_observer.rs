use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::AdapterModel,
    contract::RequiredText,
    intent::{CapabilitySnapshotV1, ExternalId, ModelId},
    limits::LIMITS,
    semantic_ingress::{
        SemanticIngressError, decode_capability_token, validate_metadata, validate_models,
        validate_permission_payload,
    },
    transport::{Frame, IncrementalFrameDecoder, validate_structure},
};

use super::super::{generated_cases::FrozenCase, wire};

pub(crate) const CASE_IDS: &[&str] = &[
    "external-id-bytes-512",
    "external-id-bytes-513",
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
        "permission-payload-bytes-65536" | "permission-payload-bytes-65537" => {
            observe_permission(frame)
        }
        _ => panic!("unregistered semantic fixture {}", case.id),
    }
}

fn observe_capability(frame: Frame) -> Value {
    match decode_capability_token(frame.as_bytes()) {
        Ok(value) => json!({
            "accepted": true,
            "normalizedDocument": if canonical_legacy_deny_all(&value) {
                "canonicalLegacyDenyAll"
            } else {
                "noncanonical"
            },
        }),
        Err(SemanticIngressError::Limit) => json!({
            "accepted": false,
            "error": "ProtocolLimit",
            "durableRowsAdded": 0,
        }),
        Err(SemanticIngressError::Invalid) => json!({
            "accepted": false,
            "error": "ProtocolInvalid",
            "durableRowsAdded": 0,
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
    match validate_metadata(metadata) {
        Ok(()) => json!({"accepted": true}),
        Err(SemanticIngressError::Limit)
            if metadata.len() > LIMITS.wire.metadata_entries as usize =>
        {
            json!({"accepted": false, "error": "ProtocolLimit"})
        }
        Err(SemanticIngressError::Limit) => json!({
            "accepted": false,
            "error": "ProtocolLimit",
            "entryLimitBreached": false,
        }),
        Err(SemanticIngressError::Invalid) => {
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
        .collect::<Result<Vec<_>, crate::acp::contract::ContractError>>();
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

fn observe_permission(frame: Frame) -> Value {
    let value = envelope_value(&frame);
    match validate_permission_payload(value.as_bytes()) {
        Ok(()) => json!({"accepted": true, "permissionGranted": false}),
        Err(_) => json!({
            "accepted": false,
            "permissionGranted": false,
            "session": "failed",
        }),
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
    value.schema_version == crate::acp::contract::SchemaVersionV1
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
