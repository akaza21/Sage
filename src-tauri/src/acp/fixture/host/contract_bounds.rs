use serde_json::{Value, json};

use crate::acp::{
    fixture::generated_cases::FrozenCase,
    intent::{NormalizedPayload, PageCursor, Prompt, ToolCategory},
    runtime::{NormalizerInput, normalize, normalized_payload_bytes},
};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "page-cursor-bytes-512",
    "page-cursor-bytes-513",
    "prompt-bytes-100000",
    "prompt-bytes-100001",
    "normalized-item-bytes-262144",
    "normalized-item-bytes-262145",
    "tool-input-output-bytes-65536",
    "tool-input-bytes-65537",
    "tool-output-bytes-65537",
    "error-bytes-8192",
    "error-bytes-8193",
    "unsupported-summary-bytes-16384",
    "unsupported-summary-bytes-16385",
];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let [step] = steps.as_slice() else {
        panic!("expected one contract-bound step");
    };
    match step["op"].as_str().unwrap() {
        "readPage" => cursor(step),
        "sendPrompt" => prompt(step),
        "normalizeItem" => normalized_item(step),
        "normalizeTool" => tool(step),
        "normalizeError" => error(step),
        "normalizeUnsupported" => unsupported(step),
        operation => panic!("unexpected contract-bound operation {operation}"),
    }
}

fn cursor(step: &Value) -> Value {
    let bytes = field_u64(step, "cursorBytes") as usize;
    if PageCursor::new("cursor", "c".repeat(bytes)).is_ok() {
        json!({"accepted": true})
    } else {
        json!({"accepted": false, "error": "ProtocolLimit"})
    }
}

fn prompt(step: &Value) -> Value {
    let bytes = field_u64(step, "bytes") as usize;
    if Prompt::new("prompt", "p".repeat(bytes)).is_ok() {
        json!({"accepted": true})
    } else {
        json!({"accepted": false, "error": "ProtocolLimit", "turnRowsAdded": 0})
    }
}

fn normalized_item(step: &Value) -> Value {
    let requested = field_u64(step, "bytes") as usize;
    let payload = payload_with_serialized_bytes(requested);
    match normalized_payload_bytes(&payload) {
        Ok(bytes) => {
            assert_eq!(bytes, requested);
            json!({"accepted": true})
        }
        Err(_) => json!({
            "accepted": false,
            "error": "ProtocolLimit",
            "retainedBytesMaximum": 262144,
        }),
    }
}

fn tool(step: &Value) -> Value {
    let input = field_u64(step, "inputBytes") as usize;
    let output = field_u64(step, "outputBytes") as usize;
    let call = normalize(NormalizerInput::ToolCall {
        category: ToolCategory::Other,
        label: "tool".into(),
        input_summary: Some("i".repeat(input)),
    });
    let result = normalize(NormalizerInput::ToolResult {
        category: ToolCategory::Other,
        label: "tool".into(),
        output_summary: Some("o".repeat(output)),
    });
    if call.is_ok() && result.is_ok() {
        json!({"accepted": true})
    } else {
        json!({"accepted": false, "error": "ProtocolLimit"})
    }
}

fn error(step: &Value) -> Value {
    let bytes = field_u64(step, "bytes") as usize;
    let result = normalize(NormalizerInput::Error {
        code: "fixture".into(),
        message: "e".repeat(bytes),
        retryable: false,
    });
    if result.is_ok() {
        json!({"accepted": true})
    } else {
        json!({"accepted": false, "retainedBytesMaximum": 8192})
    }
}

fn unsupported(step: &Value) -> Value {
    let bytes = field_u64(step, "bytes") as usize;
    let result = normalize(NormalizerInput::Unknown {
        external_kind: "fixture".into(),
        summary: "u".repeat(bytes),
    });
    if result.is_ok() {
        json!({"accepted": true})
    } else {
        json!({"accepted": false, "retainedBytesMaximum": 16384})
    }
}

fn payload_with_serialized_bytes(bytes: usize) -> NormalizedPayload {
    let empty = NormalizedPayload::Text {
        text: crate::acp::contract::SafeText::new("text", "").unwrap(),
    };
    let overhead = serde_json::to_string(&empty).unwrap().len();
    assert!(bytes >= overhead);
    NormalizedPayload::Text {
        text: crate::acp::contract::SafeText::new("text", "x".repeat(bytes - overhead)).unwrap(),
    }
}

fn field_u64(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("missing numeric field {field}"))
}
