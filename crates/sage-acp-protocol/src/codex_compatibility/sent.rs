//! Project upstream request schemas onto serializer-backed outgoing shapes.
//! Scalar domains stay conservative: a sample string does not prove that Sage
//! sends only that literal. Open members acquire their serialized value types.
use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::schema::{Evidence, SchemaBuild};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SentRequests {
    pub requests: BTreeMap<String, Vec<Value>>,
    open_objects: Vec<String>,
}

impl SentRequests {
    pub fn recorded() -> Self {
        serde_json::from_str(include_str!(
            "../../../../src-tauri/testdata/acp/codex/sent-requests.json"
        ))
        .unwrap()
    }

    pub fn project(
        &self,
        evidence: &Evidence,
        baseline: &SchemaBuild,
        schema: &Value,
        sent: &Value,
    ) -> Option<Value> {
        Projection {
            evidence,
            baseline,
            open_objects: &self.open_objects,
        }
        .project(schema, sent, "", 0)
    }
}

struct Projection<'a> {
    evidence: &'a Evidence,
    baseline: &'a SchemaBuild,
    open_objects: &'a [String],
}

impl Projection<'_> {
    fn resolve<'a>(&'a self, schema: &'a Value) -> Option<&'a Value> {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = reference.strip_prefix("#/definitions/")?;
            self.evidence
                .schemas
                .get(*self.baseline.definitions.get(name)?)
        } else {
            Some(schema)
        }
    }

    // Only selects union branches; validation constraints are retained below,
    // never inferred from the particular scalar value of an example.
    fn matches(&self, schema: &Value, value: &Value, depth: usize) -> bool {
        if depth > 64 {
            return false;
        }
        let Some(schema) = self.resolve(schema) else {
            return false;
        };
        if schema.get("$ref").is_some() {
            return self.matches(schema, value, depth + 1);
        }
        if schema == &Value::Bool(false) {
            return false;
        }
        if let Some(variants) = schema.get("enum").and_then(Value::as_array)
            && !variants.contains(value)
        {
            return false;
        }
        if let Some(kind) = schema.get("type") {
            let compatible = |kind: &Value| {
                kind.as_str().is_some_and(|kind| match kind {
                    "null" => value.is_null(),
                    "object" => value.is_object(),
                    "array" => value.is_array(),
                    "string" => value.is_string(),
                    "boolean" => value.is_boolean(),
                    "integer" => value.is_i64() || value.is_u64(),
                    "number" => value.is_number(),
                    _ => false,
                })
            };
            if !kind
                .as_array()
                .map_or_else(|| compatible(kind), |kinds| kinds.iter().any(compatible))
            {
                return false;
            }
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(variants) = schema.get(key).and_then(Value::as_array)
                && !variants
                    .iter()
                    .any(|variant| self.matches(variant, value, depth + 1))
            {
                return false;
            }
        }
        if let Some(parts) = schema.get("allOf").and_then(Value::as_array)
            && !parts
                .iter()
                .all(|part| self.matches(part, value, depth + 1))
        {
            return false;
        }
        if let Some(fields) = value.as_object() {
            if let Some(required) = schema.get("required").and_then(Value::as_array)
                && !required.iter().all(|field| {
                    field
                        .as_str()
                        .is_some_and(|field| fields.contains_key(field))
                })
            {
                return false;
            }
            if let Some(properties) = schema.get("properties").and_then(Value::as_object)
                && !properties.iter().all(|(key, schema)| {
                    fields
                        .get(key)
                        .is_none_or(|value| self.matches(schema, value, depth + 1))
                })
            {
                return false;
            }
        }
        true
    }

    fn project(&self, schema: &Value, sent: &Value, path: &str, depth: usize) -> Option<Value> {
        if depth > 64 {
            return None;
        }
        let resolved = self.resolve(schema)?;
        if resolved != schema || schema.get("$ref").is_some() {
            return self.project(resolved, sent, path, depth + 1);
        }
        let mut result = schema.as_object().cloned().unwrap_or_default();
        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(variants) = schema.get(key).and_then(Value::as_array) {
                let projected = variants
                    .iter()
                    .filter(|variant| key == "allOf" || self.matches(variant, sent, depth + 1))
                    .map(|variant| self.project(variant, sent, path, depth + 1))
                    .collect::<Option<Vec<_>>>()?;
                if projected.is_empty() {
                    return None;
                }
                result.insert(key.into(), json!(projected));
            }
        }
        // A union/allOf's branches own their object fields. Do not duplicate
        // those fields into its wrapper (notably image URL/fileId alternatives).
        let composed = ["anyOf", "oneOf", "allOf"]
            .iter()
            .any(|key| result.contains_key(*key));
        if !composed {
            result.entry("type").or_insert_with(|| {
                json!(match sent {
                    Value::Null => "null",
                    Value::Bool(_) => "boolean",
                    Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
                    Value::Number(_) => "number",
                    Value::String(_) => "string",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                })
            });
        }
        if let Some(fields) = sent.as_object()
            && (!composed || schema.get("properties").is_some())
        {
            let mut properties = Map::new();
            for (field, value) in fields {
                let previous = schema
                    .get("properties")
                    .and_then(|props| props.get(field))
                    .unwrap_or(&Value::Bool(true));
                properties.insert(
                    field.clone(),
                    self.project(previous, value, &format!("{path}/{field}"), depth + 1)?,
                );
            }
            let open = self.open_objects.iter().any(|open| open == path);
            // Removing a named field Sage sends remains a wire change even if
            // the candidate retains a catch-all for unknown members.
            result.insert(
                "x-sage-known-properties".into(),
                json!(
                    schema
                        .get("properties")
                        .and_then(Value::as_object)
                        .map(|properties| properties.keys().collect::<Vec<_>>())
                        .unwrap_or_default()
                ),
            );
            let additional = if open {
                // Variable map keys must remain accepted, including keys that
                // future schemas give a new, optional named constraint.
                json!({"anyOf": properties.values().collect::<Vec<_>>()})
            } else {
                Value::Bool(false)
            };
            result.insert(
                "required".into(),
                json!(if open {
                    Vec::new()
                } else {
                    properties.keys().collect::<Vec<_>>()
                }),
            );
            result.insert(
                "properties".into(),
                if open { json!({}) } else { json!(properties) },
            );
            result.insert("additionalProperties".into(), additional);
        }
        if let Some(items) = sent.as_array()
            && !items.is_empty()
        {
            let previous = schema.get("items").unwrap_or(&Value::Bool(true));
            let mut variants = Vec::new();
            for item in items {
                let projected = self.project(previous, item, &format!("{path}/*"), depth + 1)?;
                if !variants.contains(&projected) {
                    variants.push(projected);
                }
            }
            result.insert(
                "items".into(),
                if variants.len() == 1 {
                    variants.remove(0)
                } else {
                    json!({"anyOf": variants})
                },
            );
        }
        Some(Value::Object(result))
    }
}
