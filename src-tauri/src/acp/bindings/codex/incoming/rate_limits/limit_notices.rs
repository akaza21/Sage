//! A pushed rate-limit bucket as Sage's usage-limit notices, one per window.
//! Codex pushes numbers, not warnings, so the warning starts where Codex's
//! own TUI starts warning.

use sage_acp_protocol::{
    contract::RequiredText,
    intent::session_event::{SessionNotice, SessionNoticeLevel, UsageLimitNotice},
};

use super::RateLimitBucket;

/// Codex's TUI first warns at 75% of a window.
const WARN_PERCENT: i64 = 75;

impl RateLimitBucket {
    /// Every window is reported, all-clear included: Sage stores an
    /// all-clear only for a window it warned about.
    pub(in crate::acp::bindings::codex) fn limit_notices(&self) -> Vec<SessionNotice> {
        let bucket = self.id.as_deref().unwrap_or("default");
        self.windows
            .iter()
            .filter_map(|window| {
                let used = window.used_percent.clamp(0, 100);
                let (level, text) = if used >= 100 {
                    let text = format!("You've reached a Codex usage limit ({used}% used).");
                    (SessionNoticeLevel::Error, text)
                } else if used >= WARN_PERCENT {
                    let text = format!("You're close to a Codex usage limit ({used}% used).");
                    (SessionNoticeLevel::Warning, text)
                } else {
                    let text = format!("Your Codex usage limit is clear ({used}% used).");
                    (SessionNoticeLevel::Info, text)
                };
                let minutes = window
                    .minutes
                    .and_then(|minutes| u32::try_from(minutes).ok().filter(|m| *m > 0));
                let span = minutes.map_or_else(String::new, |minutes| minutes.to_string());
                Some(SessionNotice {
                    identity: Some(
                        RequiredText::new("noticeIdentity", format!("usage-limit:{bucket}:{span}"))
                            .ok()?,
                    ),
                    level,
                    diagnostic: false,
                    text: RequiredText::new("noticeText", text).ok()?,
                    source: Some(RequiredText::new("noticeSource", "codex").ok()?),
                    usage_limit: Some(UsageLimitNotice {
                        provider: RequiredText::new("usageProvider", "Codex").ok()?,
                        label: self
                            .name
                            .as_deref()
                            .and_then(|name| RequiredText::new("usageLabel", name).ok()),
                        used_percent: u8::try_from(used).ok(),
                        window_minutes: minutes,
                        resets_at: window.resets_at.filter(|at| *at >= 0),
                    }),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::pushed_bucket;
    use super::*;

    // An `account/rateLimits/updated` frame from codex-cli 0.167.1.
    #[test]
    fn a_pushed_bucket_reports_each_window_at_codexs_warning_levels() {
        let params = json!({"rateLimits":{"limitId":"codex","limitName":null,
            "primary":{"usedPercent":82,"windowDurationMins":300,"resetsAt":1_795_600_000},
            "secondary":{"usedPercent":100,"windowDurationMins":10080,"resetsAt":1_796_000_000},
            "credits":null,"planType":"plus","rateLimitReachedType":"rate_limit_reached"}});
        let notices = pushed_bucket(params.as_object().unwrap())
            .unwrap()
            .limit_notices();
        let shown: Vec<_> = notices
            .iter()
            .map(|notice| {
                let usage = notice.usage_limit.as_ref().unwrap();
                (
                    notice.identity.as_ref().unwrap().as_str(),
                    notice.level,
                    usage.provider.as_str(),
                    usage.used_percent,
                    usage.window_minutes,
                    usage.resets_at,
                )
            })
            .collect();
        assert_eq!(
            shown,
            vec![
                (
                    "usage-limit:codex:300",
                    SessionNoticeLevel::Warning,
                    "Codex",
                    Some(82),
                    Some(300),
                    Some(1_795_600_000)
                ),
                (
                    "usage-limit:codex:10080",
                    SessionNoticeLevel::Error,
                    "Codex",
                    Some(100),
                    Some(10080),
                    Some(1_796_000_000)
                ),
            ]
        );

        let quiet = json!({"rateLimits":{"limitId":"codex_bengalfox",
            "limitName":"GPT-5.3-Codex-Spark","primary":{"usedPercent":12,"windowDurationMins":300}}});
        let [clear] = &pushed_bucket(quiet.as_object().unwrap())
            .unwrap()
            .limit_notices()[..]
        else {
            panic!("one window")
        };
        assert_eq!(clear.level, SessionNoticeLevel::Info);
        let usage = clear.usage_limit.as_ref().unwrap();
        assert_eq!(
            usage.label.as_ref().unwrap().as_str(),
            "GPT-5.3-Codex-Spark"
        );
    }
}
