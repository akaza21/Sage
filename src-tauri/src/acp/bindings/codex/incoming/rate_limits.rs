use serde_json::{Map, Value};

use sage_acp_protocol::usage::{MAX_USAGE_WINDOWS, UsageSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Window {
    used_percent: i64,
    minutes: Option<i64>,
    resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RateLimitBucket {
    id: Option<String>,
    name: Option<String>,
    plan: Option<String>,
    windows: Vec<Window>,
    reached: bool,
}

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
        let by_id = result.get("rateLimitsByLimitId").and_then(Value::as_object);
        // Codex keys an unnamed default by its fallback id and repeats it.
        let default_key = default.and_then(|default| {
            by_id?
                .iter()
                .find(|(_, value)| value.as_object() == Some(default))
                .map(|(key, _)| key.as_str())
        });
        let mut limits = Self {
            ordinary_usage_allowed: result.get("ordinaryUsageAllowed").and_then(Value::as_bool),
            ..Self::default()
        };
        if let Some(default) = default.and_then(|value| bucket(value, default_key)) {
            limits.default_id.clone_from(&default.id);
            limits.merge(default);
        }
        for named in by_id
            .into_iter()
            .flatten()
            .filter_map(|(id, value)| bucket(value.as_object()?, Some(id)))
        {
            limits.merge(named);
        }
        (!limits.buckets.is_empty() || limits.ordinary_usage_allowed.is_some()).then_some(limits)
    }

    /// Merges a pushed bucket. One without a `limitId` is the default
    /// bucket, and until a read names the default the first pushed bucket
    /// stands in for it.
    pub(in crate::acp::bindings::codex) fn push(&mut self, mut bucket: RateLimitBucket) {
        if bucket.id.is_none() {
            bucket.id.clone_from(&self.default_id);
        }
        if self.buckets.is_empty() && self.default_id.is_none() {
            self.default_id.clone_from(&bucket.id);
        }
        self.merge(bucket);
    }

    fn merge(&mut self, bucket: RateLimitBucket) {
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

pub(in crate::acp::bindings::codex) fn pushed_bucket(
    params: &Map<String, Value>,
) -> Option<RateLimitBucket> {
    bucket(params.get("rateLimits")?.as_object()?, None)
}

/// `key` names a bucket of `rateLimitsByLimitId` that omits its `limitId`.
fn bucket(value: &Map<String, Value>, key: Option<&str>) -> Option<RateLimitBucket> {
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
        id: text(value, "limitId")
            .or_else(|| key.filter(|key| key.len() <= 128).map(str::to_owned)),
        name: text(value, "limitName"),
        plan: text(value, "planType").filter(|plan| plan != "unknown"),
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
    use sage_acp_protocol::usage::UsageWindow;
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

    fn push(value: Value) -> RateLimitBucket {
        pushed_bucket(value.as_object().unwrap()).unwrap()
    }

    #[test]
    fn a_pushed_bucket_replaces_only_its_own_limit() {
        let mut limits = RateLimits::read(
            json!({"rateLimits":{"limitId":"codex","planType":"plus",
                "primary":{"usedPercent":20,"windowDurationMins":300}},
                "rateLimitsByLimitId":{"spark":{"limitName":"Spark",
                    "primary":{"usedPercent":50,"windowDurationMins":300}}}})
            .as_object()
            .unwrap(),
        )
        .unwrap();
        limits.push(push(
            json!({"rateLimits":{"limitId":"spark","limitName":"Spark",
            "primary":{"usedPercent":90,"windowDurationMins":300}}}),
        ));
        limits.push(push(json!({"rateLimits":{
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
    fn an_unnamed_default_repeated_under_its_fallback_key_is_one_bucket() {
        let default = json!({"limitId":null,"limitName":null,"planType":"plus",
            "primary":{"usedPercent":20,"windowDurationMins":300},
            "rateLimitReachedType":"rate_limit_reached"});
        let mut limits = RateLimits::read(
            json!({"rateLimits":default,"rateLimitsByLimitId":{"codex":default}})
                .as_object()
                .unwrap(),
        )
        .unwrap();
        let snapshot = limits.snapshot();
        assert!(snapshot.limit_reached);
        assert_eq!(snapshot.windows, vec![window(None, 20, 300)]);
        limits.push(push(json!({"rateLimits":{"limitId":"codex",
            "primary":{"usedPercent":35,"windowDurationMins":300}}})));
        let snapshot = limits.snapshot();
        assert!(!snapshot.limit_reached);
        assert_eq!(snapshot.windows, vec![window(None, 35, 300)]);
    }

    #[test]
    fn before_any_read_the_first_pushed_bucket_decides_the_limit() {
        let mut limits = RateLimits::default();
        limits.push(push(json!({"rateLimits":{"limitId":"codex",
            "primary":{"usedPercent":100,"windowDurationMins":300},
            "rateLimitReachedType":"rate_limit_reached"}})));
        limits.push(push(json!({"rateLimits":{"limitId":"spark",
            "primary":{"usedPercent":10,"windowDurationMins":300}}})));
        assert!(limits.snapshot().limit_reached);
        limits.push(push(json!({"rateLimits":{
            "primary":{"usedPercent":40,"windowDurationMins":300}}})));
        let snapshot = limits.snapshot();
        assert!(!snapshot.limit_reached);
        assert_eq!(
            snapshot.windows,
            vec![window(None, 40, 300), window(None, 10, 300)]
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
