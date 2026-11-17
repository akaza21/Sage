use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{Inbound, SdkBridge};
use crate::acp::goal::NativeRpcReply;
use sage_acp_protocol::usage::UsageSnapshot;

const FIVE_HOURS: i64 = 5 * 60;
const SEVEN_DAYS: i64 = 7 * 24 * 60;

/// Claude Code's plan windows, in its `/usage` order: the label that tells
/// a model's own window apart from a plan-wide one, the window's length, and
/// whether reaching it stops every model.
const WINDOWS: [(&str, Option<&str>, i64, bool); 4] = [
    ("five_hour", None, FIVE_HOURS, true),
    ("seven_day", None, SEVEN_DAYS, true),
    ("seven_day_opus", Some("Opus"), SEVEN_DAYS, false),
    ("seven_day_sonnet", Some("Sonnet"), SEVEN_DAYS, false),
];

#[derive(Debug, Clone, PartialEq)]
pub(in crate::acp::bindings::claude::sdk) enum UsageReply {
    Limits(UsageSnapshot),
    /// Plan limits do not apply: an API key, or a cloud provider's account.
    Unsupported,
}

impl SdkBridge {
    /// Holds a main-thread model call's prompt, cached or not, plus its
    /// reply. Claude Code's synthetic error replies state no prompt and
    /// leave the count as it was; counts too large to add up make it unknown,
    /// which is stated at once.
    pub(super) fn track_context(&mut self, usage: &Value, inbound: &mut Inbound) {
        match prompt_tokens(usage) {
            Some(0) => {}
            prompt => {
                self.context_tokens = prompt.and_then(|prompt| with_reply(usage, prompt));
                if self.context_tokens.is_none() {
                    self.report_context(0, inbound);
                }
            }
        }
    }

    /// States what the context holds; zero says the count is unknown, as no
    /// model call ever sends an empty context.
    pub(super) fn report_context(&self, used: u64, inbound: &mut Inbound) {
        let mut update = json!({"sessionUpdate": "usage_update", "used": used});
        if let Some(size) = self.context_size {
            update["size"] = size.into();
        }
        inbound.acp.push(self.update(update));
    }
}

/// A turn's spend as an ACP prompt result's `usage`; `None` when its counts
/// do not add up.
pub(super) fn turn_usage(usage: &Value) -> Option<Value> {
    let total = with_reply(usage, prompt_tokens(usage)?)?;
    Some(json!({
        "inputTokens": count(usage, "input_tokens"),
        "outputTokens": count(usage, "output_tokens"),
        "cachedReadTokens": count(usage, "cache_read_input_tokens"),
        "cachedWriteTokens": count(usage, "cache_creation_input_tokens"),
        "totalTokens": total,
    }))
}

fn count(usage: &Value, key: &str) -> u64 {
    usage.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn prompt_tokens(usage: &Value) -> Option<u64> {
    [
        "input_tokens",
        "cache_read_input_tokens",
        "cache_creation_input_tokens",
    ]
    .into_iter()
    .try_fold(0_u64, |sum, key| sum.checked_add(count(usage, key)))
}

fn with_reply(usage: &Value, prompt: u64) -> Option<u64> {
    prompt.checked_add(count(usage, "output_tokens"))
}

pub(super) fn context_window(sdk: &Value) -> Option<u64> {
    sdk.get("modelUsage")?
        .as_object()?
        .values()
        .filter_map(|usage| usage.get("contextWindow").and_then(Value::as_u64))
        .max()
}

/// The read asks only for the limits; the transcript scan behind `/usage`'s
/// breakdown reads a week of local transcripts that Sage never shows.
pub(super) fn read_request() -> Value {
    json!({"subtype": "get_usage", "skip_behaviors": true})
}

/// A failed or unreadable reply is refused and leaves the last snapshot standing.
pub(super) fn answered(reply: Option<&Value>, inbound: &mut Inbound) {
    let usage = reply.and_then(usage_reply);
    inbound.native = Some(if usage.is_some() {
        NativeRpcReply::UsageRead
    } else {
        NativeRpcReply::Rejected
    });
    inbound.usage = usage;
}

fn usage_reply(reply: &Value) -> Option<UsageReply> {
    match reply
        .get("rate_limits_available")
        .and_then(Value::as_bool)?
    {
        false => Some(UsageReply::Unsupported),
        true => limits(reply).map(UsageReply::Limits),
    }
}

fn limits(reply: &Value) -> Option<UsageSnapshot> {
    let limits = reply.get("rate_limits")?.as_object()?;
    let mut snapshot = UsageSnapshot::default();
    if let Some(plan) = reply.get("subscription_type").and_then(Value::as_str) {
        snapshot.set_plan(plan);
    }
    let named = WINDOWS
        .iter()
        .filter_map(|&(key, label, minutes, plan_wide)| {
            Some((limits.get(key)?, label, minutes, plan_wide))
        });
    // A model's weekly window can be listed both by its key and by name.
    let scoped = limits
        .get("model_scoped")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|window| {
            let label = window.get("display_name")?.as_str()?;
            let listed = WINDOWS.iter().any(|&(key, known, ..)| {
                known == Some(label) && limits.get(key).is_some_and(|value| !value.is_null())
            });
            (!listed).then_some((window, Some(label), SEVEN_DAYS, false))
        });
    for (window, label, minutes, plan_wide) in named.chain(scoped) {
        let Some(used) = window.get("utilization").and_then(Value::as_f64) else {
            continue;
        };
        snapshot.limit_reached |= plan_wide && used >= 100.0;
        let resets_at = window
            .get("resets_at")
            .and_then(Value::as_str)
            .and_then(|at| OffsetDateTime::parse(at, &Rfc3339).ok())
            .map(OffsetDateTime::unix_timestamp);
        // Whole percent used, rounded down so a window short of its cap
        // never shows as fully used.
        snapshot.push(label, used.floor() as i64, Some(minutes), resets_at);
    }
    Some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sage_acp_protocol::usage::UsageWindow;

    fn window(label: Option<&str>, used: u8, minutes: u32, at: Option<i64>) -> UsageWindow {
        UsageWindow {
            label: label.map(str::to_owned),
            used_percent: used,
            window_minutes: Some(minutes),
            resets_at: at,
        }
    }

    #[test]
    fn a_plan_reports_each_window_with_its_reset_time() {
        let mut inbound = Inbound::default();
        answered(
            Some(&json!({
                "subscription_type": "max",
                "rate_limits_available": true,
                "rate_limits": {
                    "five_hour": {"utilization": 99.6, "resets_at": "2026-11-12T17:20:00Z"},
                    "seven_day": {"utilization": 55.0, "resets_at": "2026-11-18T03:37:00+00:00"},
                    "seven_day_opus": null,
                    "seven_day_sonnet": {"utilization": 100.0, "resets_at": null},
                    "model_scoped": [
                        {"display_name": "Sonnet", "utilization": 100.0, "resets_at": null},
                        {"display_name": "Fable", "utilization": 3, "resets_at": "2026-11-18T03:37:00.250Z"},
                    ],
                    "extra_usage": {"is_enabled": false},
                },
            })),
            &mut inbound,
        );
        assert_eq!(inbound.native, Some(NativeRpcReply::UsageRead));
        let Some(UsageReply::Limits(snapshot)) = inbound.usage else {
            panic!("limits");
        };
        assert_eq!(snapshot.plan.as_deref(), Some("Max"));
        assert!(!snapshot.limit_reached);
        assert_eq!(
            snapshot.windows,
            [
                window(None, 99, 300, Some(1_794_504_000)),
                window(None, 55, 10_080, Some(1_794_973_020)),
                window(Some("Sonnet"), 100, 10_080, None),
                window(Some("Fable"), 3, 10_080, Some(1_794_973_020)),
            ]
        );
    }

    #[test]
    fn an_account_without_plan_limits_is_unsupported_and_a_failed_read_is_refused() {
        let mut inbound = Inbound::default();
        answered(
            Some(
                &json!({"subscription_type": null, "rate_limits_available": false, "rate_limits": null}),
            ),
            &mut inbound,
        );
        assert_eq!(inbound.native, Some(NativeRpcReply::UsageRead));
        assert_eq!(inbound.usage, Some(UsageReply::Unsupported));
        for reply in [None, Some(json!({"rate_limits_available": true}))] {
            let mut inbound = Inbound::default();
            answered(reply.as_ref(), &mut inbound);
            assert_eq!(inbound.native, Some(NativeRpcReply::Rejected));
            assert_eq!(inbound.usage, None);
        }
    }
}
