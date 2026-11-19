//! Validate finite captures against supported baseline schema normal forms.
use super::{Projection, Value};

impl Projection<'_> {
    pub(super) fn matches(&self, schema: &Value, value: &Value, depth: usize) -> bool {
        self.validate(schema, value, depth) == Some(true)
    }

    // None means unsupported, which must propagate through exclusive unions:
    // an unproven branch may overlap an otherwise admitted branch.
    fn validate(&self, schema: &Value, value: &Value, depth: usize) -> Option<bool> {
        if depth > 64 {
            return None;
        }
        let schema = self.resolve(schema)?;
        if schema.get("$ref").is_some() {
            return self.validate(schema, value, depth + 1);
        }
        let Some(object) = schema.as_object() else {
            return Some(schema == &Value::Bool(true));
        };
        // Unknown validation keywords require new proof support. Default and
        // format are annotations in the generated JSON Schema dialect.
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "type"
                    | "enum"
                    | "default"
                    | "format"
                    | "minimum"
                    | "minLength"
                    | "anyOf"
                    | "oneOf"
                    | "allOf"
                    | "properties"
                    | "required"
                    | "additionalProperties"
                    | "patternProperties"
                    | "items"
            )
        }) {
            return None;
        }
        if let Some(variants) = schema.get("enum").and_then(Value::as_array)
            && !variants.contains(value)
        {
            return Some(false);
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
                return Some(false);
            }
        }
        if let Some(number) = value.as_f64()
            && let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
            && number < minimum
        {
            return Some(false);
        }
        if let Some(text) = value.as_str()
            && let Some(minimum) = schema.get("minLength").and_then(Value::as_u64)
            && (text.chars().count() as u64) < minimum
        {
            return Some(false);
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(variants) = schema.get(key).and_then(Value::as_array) {
                let matches = variants.iter().try_fold(0, |count, variant| {
                    self.validate(variant, value, depth + 1)
                        .map(|valid| count + usize::from(valid))
                })?;
                if matches == 0 || key == "oneOf" && matches != 1 {
                    return Some(false);
                }
            }
        }
        if let Some(parts) = schema.get("allOf").and_then(Value::as_array) {
            for part in parts {
                if !self.validate(part, value, depth + 1)? {
                    return Some(false);
                }
            }
        }
        if let Some(fields) = value.as_object() {
            if let Some(required) = schema.get("required").and_then(Value::as_array)
                && !required.iter().all(|field| {
                    field
                        .as_str()
                        .is_some_and(|field| fields.contains_key(field))
                })
            {
                return Some(false);
            }
            for (field, value) in fields {
                let named = schema.get("properties").and_then(|props| props.get(field));
                if let Some(named) = named
                    && !self.validate(named, value, depth + 1)?
                {
                    return Some(false);
                }
                let mut covered = named.is_some();
                if let Some(patterns) = schema.get("patternProperties").and_then(Value::as_object) {
                    for (pattern, shape) in patterns {
                        let applies = literal_pattern_matches(pattern, field)?;
                        if applies {
                            covered = true;
                            if !self.validate(shape, value, depth + 1)? {
                                return Some(false);
                            }
                        }
                    }
                }
                if !covered
                    && !self.validate(
                        schema
                            .get("additionalProperties")
                            .unwrap_or(&Value::Bool(true)),
                        value,
                        depth + 1,
                    )?
                {
                    return Some(false);
                }
            }
        }
        if let Some(items) = value.as_array()
            && let Some(item_schema) = schema.get("items")
        {
            for item in items {
                if !self.validate(item_schema, item, depth + 1)? {
                    return Some(false);
                }
            }
        }
        Some(true)
    }
}

// A conservative normal form, not a regex engine: literal patterns with
// optional outer anchors suffice here. More complex patterns fail closed.
fn literal_pattern_matches(pattern: &str, field: &str) -> Option<bool> {
    let start = pattern.starts_with('^');
    let pattern = pattern.strip_prefix('^').unwrap_or(pattern);
    let end = pattern.ends_with('$');
    let pattern = pattern.strip_suffix('$').unwrap_or(pattern);
    if pattern.chars().any(|c| ".^$*+?()[]{}|\\".contains(c)) {
        return None;
    }
    Some(match (start, end) {
        (true, true) => field == pattern,
        (true, false) => field.starts_with(pattern),
        (false, true) => field.ends_with(pattern),
        (false, false) => field.contains(pattern),
    })
}
