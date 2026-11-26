//! Claude Code's `rate_limit_event` as Sage's usage-limit notice: one per
//! window, so the latest report of a window replaces the last.

use serde_json::Value;

use super::{Inbound, SdkBridge};
use sage_acp_protocol::{
    contract::RequiredText,
    intent::session_event::{
        SessionNotice, SessionNoticeLevel, SessionScopedEvent, UsageLimitNotice,
    },
};

const FIVE_HOURS: u32 = 5 * 60;
const WEEK: u32 = 7 * 24 * 60;
const LAST_SECOND: i64 = 253_402_300_799;

/// Claude Code's `rateLimitType`: what the window covers when it is narrower
/// than the plan, and its length.
fn window(kind: &str) -> (Option<&'static str>, Option<u32>, &'static str) {
    match kind {
        "five_hour" => (None, Some(FIVE_HOURS), "5-hour"),
        "seven_day" => (None, Some(WEEK), "weekly"),
        "seven_day_opus" => (Some("Opus"), Some(WEEK), "weekly Opus"),
        "seven_day_sonnet" => (Some("Sonnet"), Some(WEEK), "weekly Sonnet"),
        "overage" | "seven_day_overage_included" => (Some("extra usage"), None, "extra usage"),
        _ => (None, None, "usage"),
    }
}

impl SdkBridge {
    /// `allowed` clears the window's banner; Sage keeps no all-clear for a
    /// window it never warned about.
    pub(super) fn rate_limit(sdk: &Value, inbound: &mut Inbound) {
        let info = &sdk["rate_limit_info"];
        let level = match info.get("status").and_then(Value::as_str) {
            Some("allowed") => SessionNoticeLevel::Info,
            Some("allowed_warning") => SessionNoticeLevel::Warning,
            Some("rejected") => SessionNoticeLevel::Error,
            _ => return,
        };
        let kind = info.get("rateLimitType").and_then(Value::as_str);
        if level == SessionNoticeLevel::Info && kind.is_none() {
            return;
        }
        let kind = kind.filter(|kind| kind.len() <= 64).unwrap_or("usage");
        let (label, window_minutes, span) = window(kind);
        let used_percent = info
            .get("utilization")
            .and_then(Value::as_f64)
            .filter(|used| used.is_finite())
            .map(|used| (used * 100.0).round().clamp(0.0, 100.0) as u8);
        let used = used_percent
            .map(|used| format!(" ({used}% used)"))
            .unwrap_or_default();
        let text = match level {
            SessionNoticeLevel::Error => format!("You've reached your Claude {span} limit{used}."),
            SessionNoticeLevel::Warning => {
                format!("You're close to your Claude {span} limit{used}.")
            }
            SessionNoticeLevel::Info => format!("Your Claude {span} limit is clear{used}."),
        };
        let resets_at = info
            .get("resetsAt")
            .and_then(Value::as_i64)
            .filter(|at| (0..=LAST_SECOND).contains(at));
        let (Ok(identity), Ok(text), Ok(source), Ok(provider)) = (
            RequiredText::new("noticeIdentity", format!("usage-limit:{kind}")),
            RequiredText::new("noticeText", text),
            RequiredText::new("noticeSource", "claude code"),
            RequiredText::new("usageProvider", "Claude"),
        ) else {
            return;
        };
        let label = label.and_then(|label| RequiredText::new("usageLabel", label).ok());
        inbound
            .session
            .push(SessionScopedEvent::Notice(SessionNotice {
                identity: Some(identity),
                diagnostic: false,
                level,
                text,
                source: Some(source),
                usage_limit: Some(UsageLimitNotice {
                    provider,
                    label,
                    used_percent,
                    window_minutes,
                    resets_at,
                }),
            }));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::tests::initialized;
    use super::*;

    fn limit(frame: Value) -> (SessionNoticeLevel, String, Option<UsageLimitNotice>) {
        let mut bridge = initialized();
        let inbound = bridge.inbound(frame).unwrap();
        let [SessionScopedEvent::Notice(notice)] = &inbound.session[..] else {
            panic!("one notice: {:?}", inbound.session)
        };
        let identity = notice.identity.as_ref().unwrap().as_str().to_owned();
        (notice.level, identity, notice.usage_limit.clone())
    }

    // Claude Code 2.1.330 sends these frames as is.
    #[test]
    fn a_rate_limit_event_reports_its_window_as_a_usage_limit() {
        let (level, identity, usage) = limit(json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"allowed_warning","resetsAt":1_795_600_000,
                "rateLimitType":"seven_day","utilization":0.85,"isUsingOverage":false,
                "surpassedThreshold":0.75},
            "uuid":"5f0c1d2e-8a4b-4c6d-9e7f-0a1b2c3d4e5f","session_id":tests_session()}));
        assert_eq!(level, SessionNoticeLevel::Warning);
        assert_eq!(identity, "usage-limit:seven_day");
        assert_eq!(
            usage,
            Some(UsageLimitNotice {
                provider: RequiredText::new("p", "Claude").unwrap(),
                label: None,
                used_percent: Some(85),
                window_minutes: Some(WEEK),
                resets_at: Some(1_795_600_000),
            })
        );

        let (level, identity, usage) = limit(json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"rejected","resetsAt":1_795_000_000,
                "rateLimitType":"seven_day_opus","utilization":1.0}}));
        assert_eq!(level, SessionNoticeLevel::Error);
        assert_eq!(identity, "usage-limit:seven_day_opus");
        let usage = usage.unwrap();
        assert_eq!(usage.label.unwrap().as_str(), "Opus");
        assert_eq!(usage.used_percent, Some(100));
    }

    #[test]
    fn allowed_clears_only_a_named_window() {
        let (level, identity, usage) = limit(json!({"type":"rate_limit_event",
            "rate_limit_info":{"status":"allowed","resetsAt":1_795_600_000,
                "rateLimitType":"five_hour","isUsingOverage":false}}));
        assert_eq!(level, SessionNoticeLevel::Info);
        assert_eq!(identity, "usage-limit:five_hour");
        assert_eq!(usage.unwrap().window_minutes, Some(FIVE_HOURS));

        let mut bridge = initialized();
        let quiet = bridge
            .inbound(json!({"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}))
            .unwrap();
        assert!(quiet.session.is_empty());
    }

    fn tests_session() -> &'static str {
        super::super::tests::SESSION
    }
}
