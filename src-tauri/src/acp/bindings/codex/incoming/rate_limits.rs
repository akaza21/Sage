use serde_json::{Map, Value};

use crate::acp::usage::{MAX_USAGE_WINDOWS, UsageSnapshot};

/// Reads `account/rateLimits/read` results and `account/rateLimits/updated`
/// params into the neutral usage snapshot. Unknown or malformed buckets are
/// skipped; `None` means nothing usable was reported.
pub(in crate::acp::bindings::codex) fn usage_snapshot(
    params: &Map<String, Value>,
) -> Option<UsageSnapshot> {
    let buckets: Vec<&Map<String, Value>> = match params
        .get("rateLimitsByLimitId")
        .and_then(Value::as_object)
        .filter(|buckets| !buckets.is_empty())
    {
        Some(buckets) => buckets
            .values()
            .filter_map(Value::as_object)
            .take(MAX_USAGE_WINDOWS)
            .collect(),
        None => vec![params.get("rateLimits")?.as_object()?],
    };
    let named = buckets.len() > 1;
    let mut snapshot = UsageSnapshot {
        limit_reached: params.get("ordinaryUsageAllowed") == Some(&Value::Bool(false)),
        ..UsageSnapshot::default()
    };
    for bucket in buckets {
        let label = bucket
            .get("limitName")
            .and_then(Value::as_str)
            .or_else(|| named.then(|| bucket.get("limitId")?.as_str()).flatten());
        for key in ["primary", "secondary"] {
            let Some(window) = bucket.get(key).and_then(Value::as_object) else {
                continue;
            };
            let Some(used) = window.get("usedPercent").and_then(Value::as_i64) else {
                continue;
            };
            let number = |key| window.get(key).and_then(Value::as_i64);
            snapshot.push(
                label,
                used,
                number("windowDurationMins"),
                number("resetsAt"),
            );
        }
        snapshot.limit_reached |= bucket
            .get("rateLimitReachedType")
            .is_some_and(|reached| !reached.is_null())
            || bucket.get("spendControlReached") == Some(&Value::Bool(true));
        if snapshot.plan.is_none()
            && let Some(plan) = bucket.get("planType").and_then(Value::as_str)
            && plan != "unknown"
        {
            let mut words = plan.replace('_', " ");
            if let Some(first) = words.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            snapshot.set_plan(&words);
        }
    }
    (!snapshot.windows.is_empty() || snapshot.limit_reached).then_some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::usage::UsageWindow;
    use serde_json::json;

    fn parse(value: Value) -> Option<UsageSnapshot> {
        usage_snapshot(value.as_object().unwrap())
    }

    #[test]
    fn read_result_prefers_named_buckets_and_reports_plan_and_reset() {
        let snapshot = parse(json!({
            "rateLimits": {"limitId":"codex","primary":{"usedPercent":1}},
            "rateLimitsByLimitId": {
                "codex": {
                    "limitId":"codex","limitName":null,"planType":"pro",
                    "primary":{"usedPercent":42,"windowDurationMins":300,"resetsAt":1_762_900_000},
                    "secondary":{"usedPercent":7,"windowDurationMins":10080,"resetsAt":null},
                    "credits":null,"rateLimitReachedType":null
                },
                "codex_bengalfox": {
                    "limitId":"codex_bengalfox","limitName":"GPT-5.3-Codex-Spark",
                    "primary":{"usedPercent":100,"windowDurationMins":300}
                }
            },
            "accountId": null,
            "ordinaryUsageAllowed": true
        }))
        .unwrap();
        assert_eq!(snapshot.plan.as_deref(), Some("Pro"));
        assert!(snapshot.limit_reached);
        assert_eq!(
            snapshot.windows,
            vec![
                UsageWindow {
                    label: Some("codex".into()),
                    used_percent: 42,
                    window_minutes: Some(300),
                    resets_at: Some(1_762_900_000),
                },
                UsageWindow {
                    label: Some("codex".into()),
                    used_percent: 7,
                    window_minutes: Some(10080),
                    resets_at: None,
                },
                UsageWindow {
                    label: Some("GPT-5.3-Codex-Spark".into()),
                    used_percent: 100,
                    window_minutes: Some(300),
                    resets_at: None,
                },
            ]
        );
    }

    #[test]
    fn single_bucket_update_is_unlabelled_and_reached_type_marks_the_limit() {
        let snapshot = parse(json!({"rateLimits": {
            "limitId":"codex","planType":"self_serve_business_usage_based",
            "primary":{"usedPercent":63,"windowDurationMins":300,"resetsAt":1_762_900_000},
            "secondary":null,"rateLimitReachedType":"workspace_member_usage_limit_reached"
        }}))
        .unwrap();
        assert_eq!(
            snapshot.plan.as_deref(),
            Some("Self serve business usage based")
        );
        assert!(snapshot.limit_reached);
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].label, None);
    }

    #[test]
    fn empty_or_malformed_payloads_report_nothing() {
        assert_eq!(parse(json!({})), None);
        assert_eq!(
            parse(json!({"rateLimits":{"primary":{"usedPercent":"x"}}})),
            None
        );
    }
}
