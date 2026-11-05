use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

pub fn validate_limit_fixture_coverage(
    limits_document: &str,
    coverage_document: &str,
    fixture_document: &str,
) -> Result<(), String> {
    let limits: Value = serde_json::from_str(limits_document).map_err(|error| error.to_string())?;
    let coverage: Value =
        serde_json::from_str(coverage_document).map_err(|error| error.to_string())?;
    let fixture: Value =
        serde_json::from_str(fixture_document).map_err(|error| error.to_string())?;
    let cases = fixture
        .get("cases")
        .and_then(Value::as_array)
        .ok_or("fixture cases missing")?;
    let mut cases_by_id = BTreeMap::new();
    for case in cases {
        let id = case
            .get("id")
            .and_then(Value::as_str)
            .ok_or("case id missing")?;
        if cases_by_id.insert(id, case).is_some() {
            return Err(format!("duplicate fixture case {id}"));
        }
    }
    let pairs = coverage
        .get("pairs")
        .and_then(Value::as_array)
        .ok_or("coverage pairs missing")?;
    let mut paths = BTreeSet::new();
    for pair in pairs {
        let fields = pair.as_array().ok_or("coverage pair is not an array")?;
        if fields.len() != 3 {
            return Err("coverage pair must have three fields".into());
        }
        let path = fields[0].as_str().ok_or("coverage path missing")?;
        if !paths.insert(path) {
            return Err(format!("duplicate coverage path {path}"));
        }
        let limit = resolve_path(&limits, path)?;
        let exact_id = fields[1].as_str().ok_or("exact case id missing")?;
        let over_id = fields[2].as_str().ok_or("plus-one case id missing")?;
        let exact = cases_by_id
            .get(exact_id)
            .ok_or_else(|| format!("missing exact fixture {exact_id}"))?;
        let over = cases_by_id
            .get(over_id)
            .ok_or_else(|| format!("missing plus-one fixture {over_id}"))?;
        if !contains_number(exact, limit) && !id_contains_number(exact_id, limit) {
            return Err(format!(
                "fixture {exact_id} does not contain exact limit {limit}"
            ));
        }
        if !contains_number(over, limit + 1) && !id_contains_number(over_id, limit + 1) {
            return Err(format!(
                "fixture {over_id} does not contain plus-one limit {}",
                limit + 1
            ));
        }
    }
    Ok(())
}

fn id_contains_number(id: &str, expected: u64) -> bool {
    id.split('-')
        .filter_map(|part| part.parse::<u64>().ok())
        .any(|value| value == expected)
}

fn resolve_path(document: &Value, path: &str) -> Result<u64, String> {
    let mut current = document;
    for part in path.split('.') {
        current = current
            .get(part)
            .ok_or_else(|| format!("limit path {path} is missing"))?;
    }
    current
        .as_u64()
        .ok_or_else(|| format!("limit path {path} is not an unsigned integer"))
}

fn contains_number(value: &Value, expected: u64) -> bool {
    match value {
        Value::Number(number) => number.as_u64() == Some(expected),
        Value::Array(values) => {
            let mut field_sums = BTreeMap::<&str, u64>::new();
            for object in values.iter().filter_map(Value::as_object) {
                for (key, value) in object {
                    if let Some(value) = value.as_u64() {
                        let Some(sum) = field_sums
                            .get(key.as_str())
                            .copied()
                            .unwrap_or(0)
                            .checked_add(value)
                        else {
                            continue;
                        };
                        field_sums.insert(key, sum);
                    }
                }
            }
            values.iter().any(|value| contains_number(value, expected))
                || field_sums.values().any(|sum| *sum == expected)
                || values
                    .iter()
                    .map(Value::as_u64)
                    .collect::<Option<Vec<_>>>()
                    .and_then(|values| values.into_iter().try_fold(0_u64, u64::checked_add))
                    == Some(expected)
        }
        Value::Object(values) => {
            let direct_sum = values
                .values()
                .filter_map(Value::as_u64)
                .try_fold(0_u64, u64::checked_add);
            let final_bytes = values
                .iter()
                .filter(|(key, _)| key.starts_with("final") && key.ends_with("Bytes"))
                .filter_map(|(_, value)| value.as_u64())
                .try_fold(0_u64, u64::checked_add);
            let repeated_total = values
                .iter()
                .filter(|(key, _)| key.starts_with("repeated") && key.ends_with("Bytes"))
                .find_map(|(_, value)| {
                    let repeated = value.as_object()?;
                    repeated
                        .get("bytes")?
                        .as_u64()?
                        .checked_mul(repeated.get("count")?.as_u64()?)
                })
                .zip(final_bytes)
                .and_then(|(repeated, final_bytes)| repeated.checked_add(final_bytes));
            direct_sum == Some(expected)
                || repeated_total == Some(expected)
                || values
                    .values()
                    .any(|value| contains_number(value, expected))
        }
        _ => false,
    }
}
