use serde::Serialize;
use std::sync::{Arc, Mutex, PoisonError};

use super::scheduler::RuntimeOwner;

pub const MAX_USAGE_WINDOWS: usize = 8;
const MAX_LABEL_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub label: Option<String>,
    pub used_percent: u8,
    pub window_minutes: Option<u32>,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSpend {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_tokens: u64,
    /// US dollars at the harness's own prices; `None` when it has none.
    pub cost_usd: Option<f64>,
}

impl SessionSpend {
    pub fn with_cost(mut self, cost: Option<f64>) -> Self {
        self.cost_usd = cost.filter(|cost| cost.is_finite() && *cost >= 0.0);
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub plan: Option<String>,
    pub limit_reached: bool,
    pub windows: Vec<UsageWindow>,
    pub session: Option<SessionSpend>,
}

impl UsageSnapshot {
    pub fn push(
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

    pub fn set_plan(&mut self, plan: &str) {
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
pub enum UsageState {
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
pub struct UsageCell(Mutex<UsageState>);

impl UsageCell {
    pub fn replace(&self, snapshot: UsageSnapshot) {
        *self.lock() = UsageState::Usage { snapshot };
    }

    /// The harness refused to report usage for this account.
    pub fn mark_unsupported(&self) {
        *self.lock() = UsageState::Unsupported;
    }

    /// Forgets a refusal or snapshot that an account change made stale.
    pub fn reset(&self) {
        *self.lock() = UsageState::Unavailable;
    }

    pub fn state(&self) -> UsageState {
        self.lock().clone()
    }

    pub fn supported(&self) -> bool {
        !matches!(*self.lock(), UsageState::Unsupported)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, UsageState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Old epochs cannot read or clear a new owner.
#[derive(Default)]
pub struct UsageChannel(Mutex<Option<(RuntimeOwner, Arc<UsageCell>)>>);

impl UsageChannel {
    pub fn attach(&self, owner: RuntimeOwner, cell: Arc<UsageCell>) {
        *self.lock() = Some((owner, cell));
    }

    pub fn cell(&self, owner: &RuntimeOwner) -> Option<Arc<UsageCell>> {
        self.lock()
            .as_ref()
            .filter(|(current, _)| current == owner)
            .map(|(_, cell)| Arc::clone(cell))
    }

    pub fn clear(&self, owner: &RuntimeOwner) {
        let mut slot = self.lock();
        if slot.as_ref().is_some_and(|(current, _)| current == owner) {
            *slot = None;
        }
    }

    pub fn clear_all(&self) {
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
            project_id: crate::contract::SageProjectId::parse("project-usage").unwrap(),
            chat_id: crate::contract::SageChatId::parse("chat-usage").unwrap(),
            session_id: crate::contract::SageAgentSessionId::parse("session-usage").unwrap(),
            runtime_epoch: crate::contract::RuntimeEpoch::new(epoch).unwrap(),
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
