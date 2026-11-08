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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UsageSnapshot {
    pub(crate) plan: Option<String>,
    pub(crate) limit_reached: bool,
    pub(crate) windows: Vec<UsageWindow>,
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
        let used_percent = used_percent.clamp(0, 100) as u8;
        self.limit_reached |= used_percent >= 100;
        self.windows.push(UsageWindow {
            label: label.and_then(bounded_label),
            used_percent,
            window_minutes: window_minutes.and_then(|m| u32::try_from(m).ok().filter(|m| *m > 0)),
            resets_at: resets_at.filter(|at| (0..=253_402_300_799).contains(at)),
        });
    }

    pub(crate) fn set_plan(&mut self, plan: &str) {
        self.plan = bounded_label(plan);
    }
}

fn bounded_label(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= MAX_LABEL_BYTES && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum UsageState {
    Unsupported,
    Offline,
    Unavailable,
    Usage { snapshot: UsageSnapshot },
}

/// The latest snapshot a binding observed. The binding owns it for the life
/// of its runtime and the runtime lane only borrows it.
#[derive(Default)]
pub(crate) struct UsageCell(Mutex<Option<UsageSnapshot>>);

impl UsageCell {
    pub(crate) fn replace(&self, snapshot: UsageSnapshot) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(snapshot);
    }

    pub(crate) fn snapshot(&self) -> Option<UsageSnapshot> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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
    fn windows_are_capped_clamped_and_mark_a_reached_limit() {
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
        assert!(snapshot.limit_reached);
        for _ in 0..20 {
            snapshot.push(Some("\u{7}"), -5, Some(0), Some(1_700_000_000));
        }
        assert_eq!(snapshot.windows.len(), MAX_USAGE_WINDOWS);
        assert_eq!(snapshot.windows[1].label, None);
        assert_eq!(snapshot.windows[1].used_percent, 0);
        assert_eq!(snapshot.windows[1].window_minutes, None);
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
