//! Session-scoped harness events: facts that belong to the agent session
//! rather than to one Turn (notices, the harness title, the effective model,
//! and long-lived activity such as subagents or background tasks).
//!
//! Every text value is byte-capped by its type, and [`SessionSurface`] folds
//! events into one bounded per-session state, so nothing here grows with the
//! number of events a harness streams.

use serde::{Deserialize, Serialize};

use super::{ExternalId, ModelId};
use crate::acp::contract::RequiredText;

pub(crate) const SESSION_TITLE_BYTES: usize = 256;
pub(crate) const SESSION_NOTICE_TEXT_BYTES: usize = 1024;
pub(crate) const SESSION_NOTICE_SOURCE_BYTES: usize = 128;
pub(crate) const SESSION_ACTIVITY_KIND_BYTES: usize = 64;
pub(crate) const SESSION_ACTIVITY_LABEL_BYTES: usize = 256;
pub(crate) const SESSION_ACTIVITY_PROGRESS_BYTES: usize = 512;
/// Newest notices kept per session; older ones are dropped.
pub(crate) const SESSION_NOTICES_MAX: usize = 16;
/// Live activity rows kept per session; terminal rows are evicted first.
pub(crate) const SESSION_ACTIVITIES_MAX: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionNoticeLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionNotice {
    pub level: SessionNoticeLevel,
    pub text: RequiredText<SESSION_NOTICE_TEXT_BYTES>,
    pub source: Option<RequiredText<SESSION_NOTICE_SOURCE_BYTES>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionActivityState {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl SessionActivityState {
    pub(crate) fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// Long-lived activity that may outlive the Turn that started it. `id` is
/// the harness's own identity and never leaves native code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionActivity {
    pub id: ExternalId,
    pub kind: RequiredText<SESSION_ACTIVITY_KIND_BYTES>,
    pub label: RequiredText<SESSION_ACTIVITY_LABEL_BYTES>,
    pub state: SessionActivityState,
    pub progress: Option<RequiredText<SESSION_ACTIVITY_PROGRESS_BYTES>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SessionScopedEvent {
    Notice(SessionNotice),
    /// The harness's session title; `None` when the harness cleared it.
    Title {
        title: Option<RequiredText<SESSION_TITLE_BYTES>>,
    },
    /// The model the harness reports it is actually using, which can differ
    /// from the one Sage requested (for example after a reroute).
    TurnModel {
        model_id: ModelId,
    },
    Activity(SessionActivity),
}

impl SessionScopedEvent {
    /// Whether committing this event leaves a racing send current. This is
    /// an explicit allowlist of display-only facts; a new variant must be
    /// classified here before it compiles.
    pub(crate) fn is_send_neutral(&self) -> bool {
        match self {
            Self::Notice(_) | Self::Title { .. } | Self::Activity(_) => true,
            // The effective model changes what a send runs against.
            Self::TurnModel { .. } => false,
        }
    }

    /// Whether `next` makes this event obsolete: a later title or model, or
    /// a later state of the same activity.
    pub(crate) fn superseded_by(&self, next: &Self) -> bool {
        match (self, next) {
            (Self::Title { .. }, Self::Title { .. })
            | (Self::TurnModel { .. }, Self::TurnModel { .. }) => true,
            (Self::Activity(pending), Self::Activity(next)) => pending.id == next.id,
            _ => false,
        }
    }
}

/// Session events held while no Turn can own a durable write (activation
/// and the idle span before the first Turn). Superseded events are replaced
/// and the oldest is dropped at the cap, so the buffer never grows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingSessionEvents<T = SessionScopedEvent> {
    events: Vec<T>,
}

impl<T> Default for PendingSessionEvents<T> {
    fn default() -> Self {
        Self { events: Vec::new() }
    }
}

/// Session events buffered before the first Turn of a runtime epoch.
pub(crate) const SESSION_PENDING_EVENTS_MAX: usize = 32;

impl<T> PendingSessionEvents<T> {
    pub(crate) fn push_with(&mut self, value: T, supersedes: impl Fn(&T, &T) -> bool) {
        if let Some(index) = self.events.iter().position(|old| supersedes(old, &value)) {
            self.events.remove(index);
        } else if self.events.len() == SESSION_PENDING_EVENTS_MAX {
            self.events.remove(0);
        }
        self.events.push(value);
    }

    pub(crate) fn take(&mut self) -> Vec<T> {
        std::mem::take(&mut self.events)
    }
}

impl PendingSessionEvents {
    pub(crate) fn push(&mut self, event: SessionScopedEvent) {
        self.push_with(event, SessionScopedEvent::superseded_by);
    }
}

/// How a surface change affects a racing send; the more disruptive wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SurfaceChange {
    SendNeutral,
    SendRelevant,
}

/// Bounded per-session state that session-scoped events fold into. The
/// title and notices are durable history. The model and activities describe
/// one live runtime epoch and reset when a new epoch reports.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSurface {
    pub title: Option<RequiredText<SESSION_TITLE_BYTES>>,
    pub notices: Vec<SessionNotice>,
    pub live_epoch: u64,
    pub model_id: Option<ModelId>,
    pub activities: Vec<SessionActivity>,
}

impl SessionSurface {
    /// Rejects a stored value that is over any cap.
    pub(crate) fn is_bounded(&self) -> bool {
        self.notices.len() <= SESSION_NOTICES_MAX && self.activities.len() <= SESSION_ACTIVITIES_MAX
    }

    /// Applies one event and reports what changed, or `None` if nothing did. A new epoch's first event
    /// also resets the live model and activities; dropping a model is a
    /// send-relevant change whatever the event's own kind.
    pub(crate) fn apply(
        &mut self,
        runtime_epoch: u64,
        event: &SessionScopedEvent,
    ) -> Option<SurfaceChange> {
        let mut change = None;
        if self.live_epoch != runtime_epoch {
            if self.model_id.take().is_some() {
                change = Some(SurfaceChange::SendRelevant);
            } else if !self.activities.is_empty() {
                change = Some(SurfaceChange::SendNeutral);
            }
            self.live_epoch = runtime_epoch;
            self.activities.clear();
        }
        let changed = match event {
            SessionScopedEvent::Title { title } => replace(&mut self.title, title.as_ref()),
            SessionScopedEvent::TurnModel { model_id } => {
                replace(&mut self.model_id, Some(model_id))
            }
            SessionScopedEvent::Notice(notice) => self.push_notice(notice),
            SessionScopedEvent::Activity(activity) => self.upsert_activity(activity),
        };
        if changed {
            change = change.max(Some(if event.is_send_neutral() {
                SurfaceChange::SendNeutral
            } else {
                SurfaceChange::SendRelevant
            }));
        }
        change
    }

    /// Drops the least valuable display row: the oldest notice, then the
    /// oldest finished activity, then the oldest running one. Returns false
    /// when nothing is left to drop.
    pub(crate) fn evict_oldest(&mut self) -> bool {
        if !self.notices.is_empty() {
            self.notices.remove(0);
            return true;
        }
        if self.activities.is_empty() {
            return false;
        }
        let evicted = self
            .activities
            .iter()
            .position(|row| row.state.is_terminal())
            .unwrap_or(0);
        self.activities.remove(evicted);
        true
    }

    fn push_notice(&mut self, notice: &SessionNotice) -> bool {
        if self.notices.contains(notice) {
            return false;
        }
        if self.notices.len() == SESSION_NOTICES_MAX {
            self.notices.remove(0);
        }
        self.notices.push(notice.clone());
        true
    }

    fn upsert_activity(&mut self, activity: &SessionActivity) -> bool {
        if let Some(existing) = self.activities.iter_mut().find(|row| row.id == activity.id) {
            if existing == activity {
                return false;
            }
            *existing = activity.clone();
            return true;
        }
        if self.activities.len() == SESSION_ACTIVITIES_MAX {
            let evicted = self
                .activities
                .iter()
                .position(|row| row.state.is_terminal())
                .unwrap_or(0);
            self.activities.remove(evicted);
        }
        self.activities.push(activity.clone());
        true
    }
}

fn replace<T: Clone + PartialEq>(slot: &mut Option<T>, value: Option<&T>) -> bool {
    if slot.as_ref() == value {
        return false;
    }
    *slot = value.cloned();
    true
}

#[cfg(test)]
#[path = "session_event_fixtures.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "session_event_tests.rs"]
mod tests;
