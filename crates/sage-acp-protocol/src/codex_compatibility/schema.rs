//! Conservative compatibility proof for the normal forms emitted by the Codex
//! schema generator. Used by the build-time evidence gate, never at startup.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::Value;

mod unions;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComparisonMode {
    Received,
    Sent,
    SerializedRequest,
}

impl ComparisonMode {
    fn outgoing(self) -> Self {
        if self == Self::SerializedRequest {
            self
        } else {
            Self::Sent
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Evidence {
    contract_version: String,
    exact_definitions: Vec<String>,
    open_union_definitions: Vec<String>,
    pub schemas: Vec<Value>,
    pub builds: Vec<SchemaBuild>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SchemaBuild {
    pub version: String,
    pub schema_sha256: String,
    pub wire: BTreeMap<String, usize>,
    pub definitions: BTreeMap<String, usize>,
}

impl Evidence {
    pub fn recorded() -> Self {
        serde_json::from_str(include_str!(
            "../../../../src-tauri/testdata/acp/codex/observed-wire-subset.json"
        ))
        .unwrap()
    }

    pub fn failures(&self, build: &SchemaBuild) -> Vec<String> {
        let baseline = self
            .builds
            .iter()
            .find(|build| build.version == self.contract_version)
            .expect("a real schema defines the wire contract");
        let comparison = Comparison {
            evidence: self,
            baseline,
            build,
        };
        let sent = super::sent::SentRequests::recorded();
        let mut failures: Vec<_> = baseline
            .wire
            .iter()
            .filter_map(|(method, expected)| {
                let compatible = build.wire.get(method).is_some_and(|actual| {
                    let (expected, actual) = (&self.schemas[*expected], &self.schemas[*actual]);
                    if method.starts_with("policy:") || method.starts_with("envelope:") {
                        comparison.identical(expected, actual, &mut BTreeSet::new())
                    } else if method.starts_with("client:")
                        || method.starts_with("clientNotification:")
                    {
                        sent.requests.get(method).is_some_and(|requests| {
                            !requests.is_empty()
                                && requests.iter().all(|request| {
                                    sent.project(self, baseline, expected, request).is_some_and(
                                        |projected| {
                                            comparison.compare(
                                                &projected,
                                                actual,
                                                ComparisonMode::SerializedRequest,
                                                &mut BTreeSet::new(),
                                            )
                                        },
                                    )
                                })
                        })
                    } else {
                        comparison.compatible(
                            expected,
                            actual,
                            method.starts_with("client:")
                                || method.starts_with("clientNotification:")
                                || method.starts_with("reply:"),
                            &mut BTreeSet::new(),
                        )
                    }
                });
                (!compatible).then(|| method.clone())
            })
            .collect();
        // The serializer inventory owns the request method set. An omitted
        // baseline row must not make a real outgoing method disappear here.
        failures.extend(
            sent.requests
                .keys()
                .filter(|method| !baseline.wire.contains_key(*method))
                .cloned(),
        );
        failures.sort();
        failures
    }
}

struct Comparison<'a> {
    evidence: &'a Evidence,
    baseline: &'a SchemaBuild,
    build: &'a SchemaBuild,
}

impl Comparison<'_> {
    fn resolve<'a>(&'a self, schema: &'a Value, build: &SchemaBuild) -> Option<&'a Value> {
        let name = schema
            .get("$ref")?
            .as_str()?
            .strip_prefix("#/definitions/")?;
        self.evidence.schemas.get(*build.definitions.get(name)?)
    }

    fn identical(&self, expected: &Value, actual: &Value, visited: &mut BTreeSet<String>) -> bool {
        if expected != actual {
            return false;
        }
        if let Some(reference) = expected.get("$ref").and_then(Value::as_str) {
            if !visited.insert(reference.to_owned()) {
                return true;
            }
            let (Some(left), Some(right)) = (
                self.resolve(expected, self.baseline),
                self.resolve(actual, self.build),
            ) else {
                return false;
            };
            return self.identical(left, right, visited);
        }
        match expected {
            Value::Object(object) => object
                .iter()
                .all(|(key, value)| self.identical(value, &actual[key], visited)),
            Value::Array(array) => array
                .iter()
                .enumerate()
                .all(|(index, value)| self.identical(value, &actual[index], visited)),
            _ => true,
        }
    }

    fn compatible(
        &self,
        expected: &Value,
        actual: &Value,
        sends: bool,
        visited: &mut BTreeSet<(String, String)>,
    ) -> bool {
        let mode = if sends {
            ComparisonMode::Sent
        } else {
            ComparisonMode::Received
        };
        self.compare(expected, actual, mode, visited)
    }

    fn compare(
        &self,
        expected: &Value,
        actual: &Value,
        mode: ComparisonMode,
        visited: &mut BTreeSet<(String, String)>,
    ) -> bool {
        let sends = mode != ComparisonMode::Received;
        let projected = mode == ComparisonMode::SerializedRequest;
        if projected
            && (actual == &Value::Bool(true)
                || actual.as_object().is_some_and(|object| object.is_empty()))
        {
            return true;
        }
        // Projected array items can combine several serialized forms, whereas
        // the upstream item definition may express its variants through a ref.
        if projected && expected.get("$ref").is_none() && actual.get("$ref").is_some() {
            let reference = actual["$ref"].as_str().unwrap().to_owned();
            let key = (expected.to_string(), reference);
            if !visited.insert(key.clone()) {
                return false;
            }
            let Some(actual) = self.resolve(actual, self.build) else {
                return false;
            };
            let compatible = self.compare(expected, actual, mode, visited);
            visited.remove(&key);
            return compatible;
        }
        if projected
            && let Some(variants) = expected
                .get("anyOf")
                .or_else(|| expected.get("oneOf"))
                .and_then(Value::as_array)
            && expected.as_object().is_some_and(|object| object.len() == 1)
        {
            return variants
                .iter()
                .all(|variant| self.compare(variant, actual, mode, &mut visited.clone()));
        }
        if projected
            && let Some(variants) = actual
                .get("anyOf")
                .or_else(|| actual.get("oneOf"))
                .and_then(Value::as_array)
            && actual.as_object().is_some_and(|object| object.len() == 1)
        {
            return self.union_accepts(
                expected,
                variants,
                actual.get("oneOf").is_some(),
                mode,
                visited,
            );
        }
        if let Some(reference) = expected.get("$ref").and_then(Value::as_str) {
            let Some(other) = actual.get("$ref").and_then(Value::as_str) else {
                return false;
            };
            let name = reference.strip_prefix("#/definitions/").unwrap();
            if self
                .evidence
                .exact_definitions
                .iter()
                .any(|exact| exact == name)
            {
                return self.identical(expected, actual, &mut BTreeSet::new());
            }
            if !visited.insert((reference.to_owned(), other.to_owned())) {
                return true;
            }
            let (Some(left), Some(right)) = (
                self.resolve(expected, self.baseline),
                self.resolve(actual, self.build),
            ) else {
                return false;
            };
            // Explicitly open incoming unions have a safe binding fallback for
            // new variants. All other incoming unions must retain their shape.
            let mode = if self
                .evidence
                .open_union_definitions
                .iter()
                .any(|open| open == name)
            {
                mode.outgoing()
            } else {
                mode
            };
            return self.compare(left, right, mode, visited);
        }
        if let (Some(left), Some(right)) = (expected.as_array(), actual.as_array()) {
            return left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(a, b)| self.compare(a, b, mode, visited));
        }
        let (Some(left), Some(right)) = (expected.as_object(), actual.as_object()) else {
            return expected == actual;
        };
        for (key, value) in left {
            match key.as_str() {
                "properties" => {
                    for (field, shape) in value.as_object().unwrap() {
                        let candidate =
                            right
                                .get(key)
                                .and_then(|props| props.get(field))
                                .or_else(|| {
                                    (projected
                                        && !left
                                            .get("x-sage-known-properties")
                                            .and_then(Value::as_array)
                                            .is_some_and(|known| {
                                                known
                                                    .iter()
                                                    .any(|known| known.as_str() == Some(field))
                                            }))
                                    .then(|| {
                                        right
                                            .get("additionalProperties")
                                            .unwrap_or(&Value::Bool(true))
                                    })
                                });
                        let Some(candidate) = candidate else {
                            return false;
                        };
                        if !self.compare(shape, candidate, mode, visited) {
                            return false;
                        }
                    }
                }
                "x-sage-known-properties" if projected => {}
                "additionalProperties" if projected && value == &Value::Bool(false) => {}
                "additionalProperties" if projected => {
                    if !self.compare(
                        value,
                        right.get(key).unwrap_or(&Value::Bool(true)),
                        mode.outgoing(),
                        visited,
                    ) {
                        return false;
                    }
                }
                "type" if projected && !right.contains_key(key) => {}
                "required" => {
                    let empty = Vec::new();
                    let old = value.as_array().unwrap();
                    let new = right.get(key).and_then(Value::as_array).unwrap_or(&empty);
                    let (subset, superset) = if sends { (new, old) } else { (old, new) };
                    if !subset.iter().all(|entry| superset.contains(entry)) {
                        return false;
                    }
                }
                "oneOf" | "anyOf" => {
                    let Some(candidates) = right.get(key).and_then(Value::as_array) else {
                        return false;
                    };
                    let variants = value.as_array().unwrap();
                    if !sends && variants.len() != candidates.len() {
                        return false;
                    }
                    if !variants.iter().all(|variant| {
                        self.union_accepts(variant, candidates, key == "oneOf", mode, visited)
                    }) {
                        return false;
                    }
                }
                _ => {
                    let Some(candidate) = right.get(key) else {
                        return false;
                    };
                    if !self.compare(value, candidate, mode, visited) {
                        return false;
                    }
                }
            }
        }
        // A new named property overrides additionalProperties, so even an
        // optional property can reject values previously accepted by an open
        // request object. Incoming optional fields do not have this direction.
        if sends && let Some(properties) = right.get("properties").and_then(Value::as_object) {
            for (field, shape) in properties {
                if left
                    .get("properties")
                    .and_then(|props| props.get(field))
                    .is_some()
                {
                    continue;
                }
                let unconstrained = shape == &Value::Bool(true)
                    || shape.as_object().is_some_and(|object| object.is_empty());
                // Patterns also admit named members, even when additional
                // properties are forbidden. Without proving which patterns
                // match, their accepted values cannot be narrowed safely.
                let patterned = left
                    .get("patternProperties")
                    .and_then(Value::as_object)
                    .is_some_and(|patterns| !patterns.is_empty());
                let accepted = unconstrained
                    || !patterned
                        && match left.get("additionalProperties") {
                            Some(Value::Bool(false)) => true,
                            None | Some(Value::Bool(true)) => false,
                            Some(previous) => {
                                self.compare(previous, shape, mode.outgoing(), visited)
                            }
                        };
                if !accepted {
                    return false;
                }
            }
        }
        // New validation constraints cannot silently narrow accepted requests.
        !sends
            || right.keys().all(|key| {
                left.contains_key(key)
                    || key == "properties"
                    || projected && (key == "required" || key == "additionalProperties")
            })
    }
}
