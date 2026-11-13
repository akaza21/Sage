//! Ephemeral harness usage limits. A runtime keeps only its latest snapshot;
//! nothing is persisted and no history is kept.
use serde::Serialize;
use std::sync::{Arc, Mutex, PoisonError};

use super::scheduler::RuntimeOwner;

pub(crate) const MAX_USAGE_WINDOWS: usize = 8;
const MAX_LABEL_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageWindow {
    pub(crate) label: Option<String>,
    pub(crate) used_percent: u8,
    pub(crate) window_minutes: Option<u32>,
    /// Unix seconds.
    pub(crate) resets_at: Option<i64>,
}

/// What the current session has spent so far, as its harness counts it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionSpend {
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cache_read_tokens: u64,
    pub(crate) cache_write_tokens: u64,
    pub(crate) total_tokens: u64,
    /// US dollars at the harness's own prices; `None` when it has none.
    pub(crate) cost_usd: Option<f64>,
}

impl SessionSpend {
    /// Admits a reported cost only when it is a real, non-negative amount.
    pub(crate) fn with_cost(mut self, cost: Option<f64>) -> Self {
        self.cost_usd = cost.filter(|cost| cost.is_finite() && *cost >= 0.0);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageSnapshot {
    pub(crate) plan: Option<String>,
    pub(crate) limit_reached: bool,
    pub(crate) windows: Vec<UsageWindow>,
    /// The session's own spend, for harnesses that report one.
    pub(crate) session: Option<SessionSpend>,
}

impl UsageSnapshot {
    /// Admits one window; windows past the cap are dropped.
    pub(crate) fn push(
        &mut self,
        label: Option<&str>,
        used_percent: i64,
        window_minutes: Option<i64>,
        resets_at: Option<i64>,
    ) {
        if self.windows.len() >= MAX_USAGE_WINDOWS {
            return;
        }
        self.windows.push(UsageWindow {
            label: label.and_then(bounded_label),
            used_percent: used_percent.clamp(0, 100) as u8,
            window_minutes: window_minutes.and_then(|m| u32::try_from(m).ok().filter(|m| *m > 0)),
            resets_at: resets_at.filter(|at| (0..=253_402_300_799).contains(at)),
        });
    }

    /// Names the plan as harnesses spell it (`plus`, `max`, `pro_lite`),
    /// shown with its first letter capitalized.
    pub(crate) fn set_plan(&mut self, plan: &str) {
        self.plan = bounded_label(&plan.replace('_', " ")).map(|mut plan| {
            if let Some(first) = plan.get_mut(..1) {
                first.make_ascii_uppercase();
            }
            plan
        });
    }
}

fn bounded_label(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= MAX_LABEL_BYTES && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum UsageState {
    Unsupported,
    Offline,
    #[default]
    Unavailable,
    Usage {
        snapshot: UsageSnapshot,
    },
}

/// What a binding last learned about its account's usage. The binding owns
/// it for the life of its runtime and the runtime lane only borrows it.
#[derive(Default)]
pub(crate) struct UsageCell(Mutex<UsageState>);

impl UsageCell {
    pub(crate) fn replace(&self, snapshot: UsageSnapshot) {
        *self.lock() = UsageState::Usage { snapshot };
    }

    /// The harness refused to report usage for this account.
    pub(crate) fn mark_unsupported(&self) {
        *self.lock() = UsageState::Unsupported;
    }

    /// Forgets a refusal or snapshot that an account change made stale.
    pub(crate) fn reset(&self) {
        *self.lock() = UsageState::Unavailable;
    }

    pub(crate) fn state(&self) -> UsageState {
        self.lock().clone()
    }

    pub(crate) fn supported(&self) -> bool {
        !matches!(*self.lock(), UsageState::Unsupported)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, UsageState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One slot per runtime lane. Old epochs cannot read or clear a new owner.
#[derive(Default)]
pub(crate) struct UsageChannel(Mutex<Option<(RuntimeOwner, Arc<UsageCell>)>>);

impl UsageChannel {
    pub(crate) fn attach(&self, owner: RuntimeOwner, cell: Arc<UsageCell>) {
        *self.lock() = Some((owner, cell));
    }

    pub(crate) fn cell(&self, owner: &RuntimeOwner) -> Option<Arc<UsageCell>> {
        self.lock()
            .as_ref()
            .filter(|(current, _)| current == owner)
            .map(|(_, cell)| Arc::clone(cell))
    }

    pub(crate) fn clear(&self, owner: &RuntimeOwner) {
        let mut slot = self.lock();
        if slot.as_ref().is_some_and(|(current, _)| current == owner) {
            *slot = None;
        }
    }

    pub(crate) fn clear_all(&self) {
        *self.lock() = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(RuntimeOwner, Arc<UsageCell>)>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_capped_and_clamped() {
        let mut snapshot = UsageSnapshot::default();
        snapshot.push(Some(" 5h "), 140, Some(300), Some(-1));
        assert_eq!(
            snapshot.windows[0],
            UsageWindow {
                label: Some("5h".into()),
                used_percent: 100,
                window_minutes: Some(300),
                resets_at: None,
            }
        );
        for _ in 0..20 {
            snapshot.push(Some("\u{7}"), -5, Some(0), Some(1_700_000_000));
        }
        assert_eq!(snapshot.windows.len(), MAX_USAGE_WINDOWS);
        assert_eq!(snapshot.windows[1].label, None);
        assert_eq!(snapshot.windows[1].used_percent, 0);
        assert_eq!(snapshot.windows[1].window_minutes, None);
    }

    #[test]
    fn a_session_spend_reaches_the_webview_in_its_ipc_shape() {
        let spend = SessionSpend {
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 3,
            cache_write_tokens: 4,
            total_tokens: 10,
            cost_usd: None,
        };
        let state = UsageState::Usage {
            snapshot: UsageSnapshot {
                session: Some(spend.clone().with_cost(Some(0.5))),
                ..UsageSnapshot::default()
            },
        };
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            serde_json::json!({"kind":"usage","snapshot":{
                "plan":null,"limitReached":false,"windows":[],
                "session":{"inputTokens":1,"outputTokens":2,"cacheReadTokens":3,
                    "cacheWriteTokens":4,"totalTokens":10,"costUsd":0.5}}})
        );
        for cost in [f64::NAN, f64::INFINITY, -0.01] {
            assert_eq!(spend.clone().with_cost(Some(cost)).cost_usd, None);
        }
    }

    #[test]
    fn channel_serves_only_its_current_owner() {
        let owner = |epoch| RuntimeOwner {
            project_id: crate::acp::contract::SageProjectId::parse("project-usage").unwrap(),
            chat_id: crate::acp::contract::SageChatId::parse("chat-usage").unwrap(),
            session_id: crate::acp::contract::SageAgentSessionId::parse("session-usage").unwrap(),
            runtime_epoch: crate::acp::contract::RuntimeEpoch::new(epoch).unwrap(),
        };
        let channel = UsageChannel::default();
        channel.attach(owner(1), Arc::default());
        assert!(channel.cell(&owner(2)).is_none());
        channel.clear(&owner(2));
        assert!(channel.cell(&owner(1)).is_some());
        channel.clear(&owner(1));
        assert!(channel.cell(&owner(1)).is_none());
    }
}
