use serde_json::{Value, json};

use crate::acp::{
    limits::LIMITS,
    runtime::protocol_failure_requests_tree_exit,
    transport::{FrameError, IncrementalFrameDecoder, StructuralError, validate_structure},
};

use super::super::{generated_cases::FrozenCase, wire};

pub(crate) const CASE_IDS: &[&str] = &[
    "frame-lf-split-every-byte",
    "frame-crlf-accepted",
    "frame-invalid-utf8",
    "frame-nul",
    "frame-exact-1048576",
    "frame-over-1048576",
    "frame-partial-eof",
    "frame-json-batch",
    "frame-trailing-json",
    "json-depth-32",
    "json-depth-33",
    "json-object-members-256",
    "json-object-members-257",
    "json-array-entries-512",
    "json-array-entries-513",
    "json-duplicate-decoded-key",
    "json-nodes-16384",
    "json-nodes-16385",
    "decoded-key-bytes-256",
    "decoded-key-bytes-257",
    "method-bytes-128",
    "method-bytes-129",
    "numeric-token-bytes-64",
    "numeric-token-bytes-65",
];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let (stdout, _) = wire::capture_steps(&steps).unwrap();
    let split_every_byte = steps
        .iter()
        .any(|step| step.get("split").and_then(Value::as_str) == Some("everyByte"));
    let observed = decode(&stdout, split_every_byte);

    match case.id {
        "frame-lf-split-every-byte" | "frame-crlf-accepted" => json!({
            "terminal": if observed.accepted() { "ready" } else { "failed" },
            "protocolViolation": !observed.accepted(),
        }),
        "frame-invalid-utf8" | "frame-nul" => json!({
            "terminal": "failed",
            "protocolViolation": observed.protocol_violation(),
            "treeMustExit": observed.protocol_violation()
                && protocol_failure_requests_tree_exit(),
        }),
        "frame-exact-1048576" => json!({
            "protocolViolation": observed.protocol_violation(),
            "allocationBytesMaximum": LIMITS.wire.frame_bytes,
        }),
        "frame-over-1048576" => json!({
            "terminal": "failed",
            "protocolViolation": observed.protocol_violation(),
            "allocationBytesMaximum": LIMITS.wire.frame_bytes,
        }),
        "frame-partial-eof"
        | "frame-json-batch"
        | "frame-trailing-json"
        | "json-duplicate-decoded-key" => json!({
            "terminal": "failed",
            "protocolViolation": observed.protocol_violation(),
        }),
        "json-depth-32"
        | "json-object-members-256"
        | "json-array-entries-512"
        | "json-nodes-16384"
        | "decoded-key-bytes-256"
        | "method-bytes-128"
        | "numeric-token-bytes-64" => json!({
            "structuralValidation": if observed.accepted() {
                "accepted"
            } else {
                "limitExceeded"
            },
        }),
        "json-depth-33"
        | "json-object-members-257"
        | "json-array-entries-513"
        | "json-nodes-16385"
        | "decoded-key-bytes-257"
        | "method-bytes-129"
        | "numeric-token-bytes-65" => json!({
            "structuralValidation": if observed.structural_limit() {
                "limitExceeded"
            } else {
                "accepted"
            },
            "terminal": if observed.protocol_violation() {
                "failed"
            } else {
                "ready"
            },
        }),
        _ => panic!("unregistered wire fixture {}", case.id),
    }
}

struct Observation {
    frame_error: Option<FrameError>,
    structural_error: Option<StructuralError>,
    frames: usize,
}

impl Observation {
    fn accepted(&self) -> bool {
        self.frame_error.is_none() && self.structural_error.is_none() && self.frames == 1
    }

    fn protocol_violation(&self) -> bool {
        !self.accepted()
    }

    fn structural_limit(&self) -> bool {
        matches!(
            self.structural_error,
            Some(
                StructuralError::ArrayTooLarge
                    | StructuralError::DecodedKeyTooLarge
                    | StructuralError::Depth
                    | StructuralError::MethodTooLarge
                    | StructuralError::Nodes
                    | StructuralError::NumericTokenTooLarge
                    | StructuralError::ObjectTooLarge
            )
        )
    }
}

fn decode(bytes: &[u8], split_every_byte: bool) -> Observation {
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frames = Vec::new();
    let chunk_bytes = if split_every_byte { 1 } else { 8 * 1024 };
    let mut frame_error = None;
    for chunk in bytes.chunks(chunk_bytes) {
        match decoder.push_chunk(chunk) {
            Ok(Some(frame)) => frames.push(frame),
            Ok(None) => {}
            Err(error) => {
                frame_error = Some(error);
                break;
            }
        }
        while decoder.has_pending_input() {
            match decoder.drain_pending() {
                Ok(Some(frame)) => frames.push(frame),
                Ok(None) => {}
                Err(error) => {
                    frame_error = Some(error);
                    break;
                }
            }
        }
        if frame_error.is_some() {
            break;
        }
    }
    if frame_error.is_none() {
        match decoder.finish() {
            Ok(Some(frame)) => frames.push(frame),
            Ok(None) => {}
            Err(error) => frame_error = Some(error),
        }
    }
    let structural_error = if frame_error.is_none() && frames.len() == 1 {
        validate_structure(&frames[0]).err()
    } else {
        None
    };
    Observation {
        frame_error,
        structural_error,
        frames: frames.len(),
    }
}
