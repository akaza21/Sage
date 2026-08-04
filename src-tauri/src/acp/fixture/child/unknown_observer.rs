use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{AdapterEvent, AdapterRuntimeError, HarnessAdapter, ResourceMapper},
    contract::{RuntimeEpoch, SageAgentSessionId},
    fixture::adapter::FixtureAdapter,
    intent::{ExternalId, SafeResourceRef, UnavailableResourceReason},
    transport::{Frame, IncrementalFrameDecoder, validate_structure},
};

use super::super::{generated_cases::FrozenCase, wire};

pub(crate) const CASE_IDS: &[&str] = &[
    "rpc-unknown-bounded-notification",
    "rpc-unknown-request",
    "collaboration-unknown-operation",
];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let (stdout, _) = wire::capture_steps(&steps).unwrap();
    let frame = decode_one(&stdout);
    let mut adapter = FixtureAdapter::new(
        [7; 32],
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("fixture-session").unwrap(),
        ExternalId::new("externalSession", "fixture-external-session").unwrap(),
    )
    .unwrap();
    let capabilities_before = adapter.capabilities();
    let result = adapter.ingest(&frame, &Mapper);
    let capabilities_after = adapter.capabilities();
    match case.id {
        "rpc-unknown-bounded-notification" => json!({
            "normalizedItem": if matches!(
                result.as_ref().map(|batch| batch.events()),
                Ok([AdapterEvent::Unsupported(_)])
            ) {
                "unsupported"
            } else {
                "invalid"
            },
            "capabilityChange": capabilities_before != capabilities_after,
        }),
        "rpc-unknown-request" => json!({
            "terminal": if matches!(result, Err(AdapterRuntimeError::Frame)) {
                "failed"
            } else {
                "ready"
            },
            "permissionGranted": false,
        }),
        "collaboration-unknown-operation" => json!({
            "normalizedItem": if matches!(
                result.as_ref().map(|batch| batch.events()),
                Ok([AdapterEvent::Unsupported(_)])
            ) {
                "unsupported"
            } else {
                "invalid"
            },
            "workerControlEnabled": false,
        }),
        _ => panic!("unregistered unknown fixture {}", case.id),
    }
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

fn decode_one(bytes: &[u8]) -> Frame {
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
    validate_structure(&frame).unwrap();
    frame
}
