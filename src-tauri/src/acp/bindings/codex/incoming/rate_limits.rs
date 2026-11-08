use serde_json::{Map, Value};

use crate::acp::usage::{MAX_USAGE_WINDOWS, UsageSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Window {
    used_percent: i64,
    minutes: Option<i64>,
    resets_at: Option<i64>,
}

/// One metered Codex limit bucket, keyed by its `limitId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RateLimitBucket {
    id: Option<String>,
    name: Option<String>,
    plan: Option<String>,
    windows: Vec<Window>,
    reached: bool,
}

/// The account's buckets as of the last read, with pushed buckets merged in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RateLimits {
    buckets: Vec<RateLimitBucket>,
    default_id: Option<String>,
    ordinary_usage_allowed: Option<bool>,
}

impl RateLimits {
    /// An `account/rateLimits/read` result; `None` when nothing usable was
    /// reported.
    pub(in crate::acp::bindings::codex) fn read(result: &Map<String, Value>) -> Option<Self> {
        let default = result.get("rateLimits").and_then(Value::as_object);
        let mut limits = Self {
            default_id: default.and_then(|bucket| text(bucket, "limitId")),
            ordinary_usage_allowed: result.get("ordinaryUsageAllowed").and_then(Value::as_bool),
            ..Self::default()
        };
        if let Some(default) = default.and_then(bucket) {
            limits.merge(default);
        }
        for named in result
            .get("rateLimitsByLimitId")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(Map::values)
            .filter_map(Value::as_object)
            .filter_map(bucket)
        {
            limits.merge(named);
        }
        (!limits.buckets.is_empty() || limits.ordinary_usage_allowed.is_some()).then_some(limits)
    }

    /// Replaces the bucket with the same `limitId`, or admits a new one while
    /// under the cap.
    pub(in crate::acp::bindings::codex) fn merge(&mut self, bucket: RateLimitBucket) {
        if let Some(known) = self.buckets.iter_mut().find(|known| known.id == bucket.id) {
            *known = bucket;
        } else if self.buckets.len() < MAX_USAGE_WINDOWS {
            self.buckets.push(bucket);
        }
    }

    pub(in crate::acp::bindings::codex) fn snapshot(&self) -> UsageSnapshot {
        let default = self
            .buckets
            .iter()
            .find(|bucket| bucket.id == self.default_id);
        let mut snapshot = UsageSnapshot {
            limit_reached: self.ordinary_usage_allowed.map_or_else(
                || default.is_some_and(|bucket| bucket.reached),
                |allowed| !allowed,
            ),
            ..UsageSnapshot::default()
        };
        if let Some(plan) = default
            .into_iter()
            .chain(&self.buckets)
            .find_map(|bucket| bucket.plan.as_deref())
        {
            snapshot.set_plan(plan);
        }
        for bucket in default.into_iter().chain(
            self.buckets
                .iter()
                .filter(|bucket| bucket.id != self.default_id),
        ) {
            for window in &bucket.windows {
                snapshot.push(
                    bucket.name.as_deref(),
                    window.used_percent,
                    window.minutes,
                    window.resets_at,
                );
            }
        }
        snapshot
    }
}

/// The bucket an `account/rateLimits/updated` notification carries.
pub(in crate::acp::bindings::codex) fn pushed_bucket(
    params: &Map<String, Value>,
) -> Option<RateLimitBucket> {
    bucket(params.get("rateLimits")?.as_object()?)
}

fn bucket(value: &Map<String, Value>) -> Option<RateLimitBucket> {
    let windows: Vec<Window> = ["primary", "secondary"]
        .into_iter()
        .filter_map(|key| {
            let window = value.get(key)?.as_object()?;
            let number = |key| window.get(key).and_then(Value::as_i64);
            Some(Window {
                used_percent: number("usedPercent")?,
                minutes: number("windowDurationMins"),
                resets_at: number("resetsAt"),
            })
        })
        .collect();
    let reached = value
        .get("rateLimitReachedType")
        .is_some_and(|reached| !reached.is_null())
        || value.get("spendControlReached") == Some(&Value::Bool(true));
    (!windows.is_empty() || reached).then(|| RateLimitBucket {
        id: text(value, "limitId"),
        name: text(value, "limitName"),
        plan: text(value, "planType")
            .filter(|plan| plan != "unknown")
            .map(|plan| {
                let mut words = plan.replace('_', " ");
                if let Some(first) = words.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                words
            }),
        windows,
        reached,
    })
}

fn text(value: &Map<String, Value>, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| text.len() <= 128)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::usage::UsageWindow;
    use serde_json::json;

    fn read(value: Value) -> Option<UsageSnapshot> {
        RateLimits::read(value.as_object().unwrap()).map(|limits| limits.snapshot())
    }

    fn window(label: Option<&str>, used_percent: u8, minutes: u32) -> UsageWindow {
        UsageWindow {
            label: label.map(str::to_owned),
            used_percent,
            window_minutes: Some(minutes),
            resets_at: None,
        }
    }

    #[test]
    fn read_lists_the_default_bucket_first_and_labels_only_named_buckets() {
        let snapshot = read(json!({
            "rateLimits": {"limitId":"codex","primary":{"usedPercent":1}},
            "rateLimitsByLimitId": {
                "codex_bengalfox": {
                    "limitId":"codex_bengalfox","limitName":"GPT-5.3-Codex-Spark",
                    "primary":{"usedPercent":100,"windowDurationMins":300}
                },
                "codex": {
                    "limitId":"codex","limitName":null,"planType":"pro",
                    "primary":{"usedPercent":42,"windowDurationMins":300,"resetsAt":1_762_900_000},
                    "secondary":{"usedPercent":7,"windowDurationMins":10080,"resetsAt":null},
                    "credits":null,"rateLimitReachedType":null
                },
                "codex_unnamed": {"limitId":"codex_unnamed","primary":{"usedPercent":3,"windowDurationMins":60}}
            },
            "accountId": null,
            "ordinaryUsageAllowed": null
        }))
        .unwrap();
        assert_eq!(snapshot.plan.as_deref(), Some("Pro"));
        assert!(
            !snapshot.limit_reached,
            "a full side bucket shows in its bar, not as the account's limit"
        );
        assert_eq!(
            snapshot.windows,
            vec![
                UsageWindow {
                    resets_at: Some(1_762_900_000),
                    ..window(None, 42, 300)
                },
                window(None, 7, 10080),
                window(Some("GPT-5.3-Codex-Spark"), 100, 300),
                window(None, 3, 60),
            ]
        );
    }

    #[test]
    fn ordinary_usage_allowed_alone_decides_the_account_limit() {
        let reached_default = json!({"limitId":"codex","primary":{"usedPercent":100},
            "rateLimitReachedType":"rate_limit_reached"});
        assert!(
            !read(json!({"rateLimits":reached_default,"ordinaryUsageAllowed":true}))
                .unwrap()
                .limit_reached
        );
        assert!(
            read(json!({"rateLimits":reached_default}))
                .unwrap()
                .limit_reached
        );
        assert!(
            read(
                json!({"rateLimits":{"limitId":"codex","primary":{"usedPercent":5}},
                "ordinaryUsageAllowed":false})
            )
            .unwrap()
            .limit_reached
        );
    }

    #[test]
    fn a_pushed_bucket_replaces_only_its_own_limit() {
        let mut limits = RateLimits::read(
            json!({"rateLimits":{"limitId":"codex","planType":"plus",
                "primary":{"usedPercent":20,"windowDurationMins":300}},
                "rateLimitsByLimitId":{"spark":{"limitId":"spark","limitName":"Spark",
                    "primary":{"usedPercent":50,"windowDurationMins":300}}}})
            .as_object()
            .unwrap(),
        )
        .unwrap();
        let push = |value: Value| pushed_bucket(value.as_object().unwrap()).unwrap();
        limits.merge(push(
            json!({"rateLimits":{"limitId":"spark","limitName":"Spark",
            "primary":{"usedPercent":90,"windowDurationMins":300}}}),
        ));
        limits.merge(push(json!({"rateLimits":{"limitId":"codex",
            "planType":"self_serve_business_usage_based",
            "primary":{"usedPercent":63,"windowDurationMins":300},
            "rateLimitReachedType":"workspace_member_usage_limit_reached"}})));
        let snapshot = limits.snapshot();
        assert_eq!(
            snapshot.plan.as_deref(),
            Some("Self serve business usage based")
        );
        assert!(snapshot.limit_reached);
        assert_eq!(
            snapshot.windows,
            vec![window(None, 63, 300), window(Some("Spark"), 90, 300)]
        );
    }

    #[test]
    fn empty_or_malformed_payloads_report_nothing() {
        assert_eq!(read(json!({})), None);
        assert_eq!(
            read(json!({"rateLimits":{"primary":{"usedPercent":"x"}}})),
            None
        );
        assert_eq!(
            pushed_bucket(json!({"rateLimits":{}}).as_object().unwrap()),
            None
        );
    }
}
