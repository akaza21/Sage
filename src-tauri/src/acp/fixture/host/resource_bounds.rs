use serde_json::{Value, json};

use crate::acp::{
    fixture::generated_cases::FrozenCase,
    resource_mapper::{ResourceMapError, validate_native_path_bytes},
};
use sage_acp_protocol::{contract::SageRootId, intent::SafeResourceRef};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "absolute-path-bytes-32768",
    "absolute-path-bytes-32769",
    "relative-path-bytes-16384",
    "relative-path-bytes-16385",
];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let [step] = steps.as_slice() else {
        panic!("expected one resource-bound step");
    };
    assert_eq!(step["op"], "mapNativePath");
    if let Some(bytes) = step.get("absolutePathBytes").and_then(Value::as_u64) {
        return match validate_native_path_bytes(&"p".repeat(bytes as usize)) {
            Ok(()) => json!({
                "preFilesystemByteValidationAccepted": true,
                "webviewAbsolutePath": false,
            }),
            Err(ResourceMapError::PathBytes) => json!({
                "accepted": false,
                "error": "ResourceLimit",
                "webviewAbsolutePath": false,
            }),
            Err(error) => panic!("unexpected native path error {error:?}"),
        };
    }
    let bytes = step["relativePathBytes"].as_u64().unwrap() as usize;
    if SafeResourceRef::new(
        SageRootId::parse("fixture-root").unwrap(),
        "r".repeat(bytes),
    )
    .is_ok()
    {
        json!({"accepted": true, "resourceRef": true})
    } else {
        json!({"accepted": false, "error": "ResourceLimit", "resourceRef": false})
    }
}
