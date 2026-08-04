use std::ffi::OsString;

use serde_json::{Value, json};

use crate::acp::{
    fixture::generated_cases::FrozenCase,
    process::{BoundedEnvironment, BoundedOsArgs, LaunchError},
};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "process-arguments-32",
    "process-arguments-33",
    "process-argument-bytes-8192",
    "process-argument-bytes-8193",
    "process-argument-total-32768",
    "process-argument-total-32769",
    "environment-entries-64",
    "environment-entries-65",
    "environment-key-bytes-256",
    "environment-key-bytes-257",
    "environment-value-bytes-8192",
    "environment-value-bytes-8193",
    "environment-total-bytes-32768",
    "environment-total-bytes-32769",
];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let [step] = steps.as_slice() else {
        panic!("expected one process-bound step");
    };
    assert_eq!(step["op"], "launchShape");
    match case.id {
        id if id.starts_with("process-") => execute_arguments(id, step),
        id if id.starts_with("environment-") => execute_environment(id, step),
        id => panic!("unexpected process-bound case {id}"),
    }
}

fn execute_arguments(id: &str, step: &Value) -> Value {
    let lengths = if let Some(count) = step.get("argumentCount").and_then(Value::as_u64) {
        let total = field_u64(step, "argumentBytesTotal");
        assert_eq!(count, total);
        vec![1; count as usize]
    } else {
        step["argumentBytes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap())
            .collect()
    };
    let args = lengths
        .into_iter()
        .map(|bytes| OsString::from("a".repeat(bytes as usize)))
        .collect();
    match BoundedOsArgs::new(args) {
        Ok(_) => json!({"accepted": true}),
        Err(LaunchError::ArgumentLimit) => match id {
            "process-argument-total-32769" => json!({
                "accepted": false,
                "perArgumentLimitBreached": false,
                "processCreated": false,
            }),
            _ => json!({"accepted": false, "processCreated": false}),
        },
        Err(error) => panic!("unexpected argument error {error:?}"),
    }
}

fn execute_environment(id: &str, step: &Value) -> Value {
    let pairs = if let Some(count) = step.get("environmentEntries").and_then(Value::as_u64) {
        let total = field_u64(step, "environmentBytesTotal");
        assert_eq!(total, count * 2);
        one_byte_pairs(count)
    } else if let Some(key) = step.get("environmentKeyBytes").and_then(Value::as_u64) {
        vec![(
            "k".repeat(key as usize),
            "v".repeat(field_u64(step, "environmentValueBytes") as usize),
        )]
    } else {
        step["environmentPairBytes"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let total = value.as_u64().unwrap();
                let key = one_byte_key(index);
                (key, "v".repeat(total.saturating_sub(1) as usize))
            })
            .collect()
    };
    let environment = pairs
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect();
    match BoundedEnvironment::new(environment) {
        Ok(_) => json!({"accepted": true}),
        Err(LaunchError::EnvironmentLimit) => match id {
            "environment-total-bytes-32769" => json!({
                "accepted": false,
                "perEntryLimitBreached": false,
                "processCreated": false,
            }),
            _ => json!({"accepted": false, "processCreated": false}),
        },
        Err(error) => panic!("unexpected environment error {error:?}"),
    }
}

fn one_byte_pairs(count: u64) -> Vec<(String, String)> {
    (0..count as usize)
        .map(|index| (one_byte_key(index), "v".into()))
        .collect()
}

fn one_byte_key(index: usize) -> String {
    const KEYS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789!#$%&'()*+,-./:;<>?@[\\]^_`{|}~";
    char::from(*KEYS.get(index).expect("fixture key remains one byte")).to_string()
}

fn field_u64(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("missing numeric field {field}"))
}
