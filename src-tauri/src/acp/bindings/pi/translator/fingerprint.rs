use std::hash::{Hash, Hasher};

use serde_json::Value;

use crate::acp::bindings::pi::decode::Response;

pub(super) fn response_fingerprint(response: &Response) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    response.command.hash(&mut hasher);
    response.success.hash(&mut hasher);
    hash_json(&response.data, &mut hasher);
    hasher.finish()
}

fn hash_json(value: &Value, hasher: &mut impl Hasher) {
    std::mem::discriminant(value).hash(hasher);
    match value {
        Value::Null => {}
        Value::Bool(value) => value.hash(hasher),
        Value::Number(value) => value.to_string().hash(hasher),
        Value::String(value) => value.hash(hasher),
        Value::Array(values) => values.iter().for_each(|value| hash_json(value, hasher)),
        Value::Object(values) => values.iter().for_each(|(key, value)| {
            key.hash(hasher);
            hash_json(value, hasher);
        }),
    }
}
