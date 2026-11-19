//! Exclusive request unions require both admission and non-overlap.
use super::{BTreeSet, Comparison, ComparisonMode, Value};

impl Comparison<'_> {
    pub(super) fn exclusive(
        &self,
        expected: &Value,
        candidates: &[Value],
        visited: &BTreeSet<(String, String)>,
    ) -> bool {
        let admitted = candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                self.compare(
                    expected,
                    candidate,
                    ComparisonMode::SerializedRequest,
                    &mut visited.clone(),
                )
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [selected] = admitted.as_slice() else {
            return false;
        };
        candidates
            .iter()
            .enumerate()
            .all(|(index, candidate)| index == *selected || self.disjoint(expected, candidate, 0))
    }

    fn disjoint(&self, expected: &Value, actual: &Value, depth: usize) -> bool {
        if depth > 64 {
            return false;
        }
        if let Some(resolved) = self.resolve(expected, self.baseline) {
            return self.disjoint(resolved, actual, depth + 1);
        }
        if let Some(resolved) = self.resolve(actual, self.build) {
            return self.disjoint(expected, resolved, depth + 1);
        }
        if expected == &Value::Bool(false) || actual == &Value::Bool(false) {
            return true;
        }
        for (union, other, reversed) in [(expected, actual, false), (actual, expected, true)] {
            if let Some(variants) = union
                .get("anyOf")
                .or_else(|| union.get("oneOf"))
                .and_then(Value::as_array)
                && variants.iter().all(|variant| {
                    if reversed {
                        self.disjoint(other, variant, depth + 1)
                    } else {
                        self.disjoint(variant, other, depth + 1)
                    }
                })
            {
                return true;
            }
        }
        if let (Some(left), Some(right)) = (expected.get("type"), actual.get("type")) {
            let types = |value: &Value| {
                value
                    .as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![value.clone()])
            };
            let left = types(left);
            let right = types(right);
            if left.iter().all(|a| {
                right.iter().all(|b| {
                    a != b
                        && !matches!(
                            (a.as_str(), b.as_str()),
                            (Some("integer"), Some("number")) | (Some("number"), Some("integer"))
                        )
                })
            }) {
                return true;
            }
        }
        if let (Some(left), Some(right)) = (
            expected.get("enum").and_then(Value::as_array),
            actual.get("enum").and_then(Value::as_array),
        ) && left.iter().all(|value| !right.contains(value))
        {
            return true;
        }
        // Every property of a projected closed object is a serialized member.
        // A different discriminator, or an absent required member, separates
        // the branches even when the candidate does not declare its type.
        if expected.get("type").and_then(Value::as_str) == Some("object") {
            if let Some(required) = actual.get("required").and_then(Value::as_array)
                && expected.get("additionalProperties") == Some(&Value::Bool(false))
                && required.iter().any(|field| {
                    field.as_str().is_some_and(|field| {
                        expected
                            .get("properties")
                            .and_then(|props| props.get(field))
                            .is_none()
                    })
                })
            {
                return true;
            }
            if let Some(required) = expected.get("required").and_then(Value::as_array) {
                for field in required.iter().filter_map(Value::as_str) {
                    if let (Some(left), Some(right)) = (
                        expected
                            .get("properties")
                            .and_then(|props| props.get(field)),
                        actual.get("properties").and_then(|props| props.get(field)),
                    ) && self.disjoint(left, right, depth + 1)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}
