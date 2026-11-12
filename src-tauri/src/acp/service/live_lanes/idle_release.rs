//! Releases the harness processes of Chats the user has left.
//!
//! A live runtime with no Turn running is parked through the graceful close
//! an evicted lane takes once its Chat has been out of view and idle for
//! `HIDDEN_IDLE`, or idle for `SHOWN_IDLE` even in view. The next send
//! resumes the session. Only runtimes eviction could park are parked: a
//! resumable session whose runtime has completed a Turn.
//!
//! The WebView reports the Chats it shows with the visibility it reports for
//! turn notices. Until its first report, no Chat counts as out of view and
//! only the `SHOWN_IDLE` rule applies.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::acp::{contract::SageChatId, scheduler::RuntimeOwner};

use super::{AcpService, ActivationClaim, explicit_close_deadline};

/// Idle time after which a Chat out of view is released.
const HIDDEN_IDLE: Duration = Duration::from_secs(2 * 60);
/// Idle time after which a Chat still in view is released.
const SHOWN_IDLE: Duration = Duration::from_secs(15 * 60);

/// Which Chats are in view, and since when each live one has not been.
#[derive(Debug, Default)]
pub(crate) struct IdleRelease {
    view: Mutex<View>,
    /// Held for a whole sweep, so stopping waits for a park in flight.
    sweep: Mutex<()>,
    stopped: AtomicBool,
}

#[derive(Debug, Default)]
struct View {
    /// None until the WebView first reports.
    shown: Option<HashSet<SageChatId>>,
    /// When each Chat left the view; kept only for Chats with a live runtime
    /// or that just left.
    hidden_since: HashMap<SageChatId, Instant>,
}

impl View {
    /// Whether a live runtime idle since `idle_since` is due for release.
    fn due(&mut self, chat: &SageChatId, idle_since: Instant, now: Instant) -> bool {
        let idle = now.saturating_duration_since(idle_since);
        if idle >= SHOWN_IDLE {
            return true;
        }
        let Some(shown) = &self.shown else {
            return false;
        };
        if shown.contains(chat) {
            return false;
        }
        let hidden = self.hidden_since.entry(chat.clone()).or_insert(now);
        idle >= HIDDEN_IDLE && now.saturating_duration_since(*hidden) >= HIDDEN_IDLE
    }
}

impl IdleRelease {
    /// Stops sweeping; a park already running finishes first.
    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        drop(self.sweep.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// Whether sweeping has stopped.
    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
}

impl AcpService {
    /// How often idle runtimes are looked at.
    pub(crate) const IDLE_SWEEP_INTERVAL: Duration = Duration::from_secs(15);

    /// Records the Chats the WebView shows; every other Chat is out of view
    /// from now.
    pub(crate) fn set_shown_chats(&self, chats: impl IntoIterator<Item = SageChatId>) {
        let now = Instant::now();
        let shown: HashSet<SageChatId> = chats.into_iter().collect();
        let mut view = self
            .idle_release
            .view
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for left in view.shown.take().unwrap_or_default() {
            if !shown.contains(&left) {
                view.hidden_since.entry(left).or_insert(now);
            }
        }
        view.hidden_since.retain(|key, _| !shown.contains(key));
        view.shown = Some(shown);
    }

    /// Parks every live runtime due for release, at `now`. Answers how many
    /// were parked, or None once sweeping has stopped.
    pub(crate) fn release_idle_runtimes(&self, now: Instant) -> Option<usize> {
        let release = &self.idle_release;
        let _sweep = release.sweep.lock().unwrap_or_else(PoisonError::into_inner);
        if release.is_stopped() {
            return None;
        }
        let due = self.idle_runtimes_due(now);
        let deadline = explicit_close_deadline();
        let mut parked = 0;
        for (index, owner, idle_since) in due {
            if release.is_stopped() || Instant::now() >= deadline {
                break;
            }
            parked += usize::from(self.park_idle_lane(index, &owner, idle_since, deadline));
        }
        Some(parked)
    }

    /// The lanes whose idle runtime is due, with the idle mark each was due
    /// at. Out-of-view marks of Chats without a live runtime are dropped.
    fn idle_runtimes_due(&self, now: Instant) -> Vec<(usize, RuntimeOwner, Instant)> {
        let active: Vec<_> = self
            .lanes
            .iter()
            .map(|lane| lane.scheduler.active())
            .collect();
        let mut view = self
            .idle_release
            .view
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        view.hidden_since.retain(|chat, _| {
            active
                .iter()
                .flatten()
                .any(|active| &active.tree.chat_id == chat)
        });
        active
            .into_iter()
            .enumerate()
            .filter_map(|(index, active)| {
                let active = active?;
                let idle_since = active.idle_since?;
                view.due(&active.tree.chat_id, idle_since, now).then_some((
                    index,
                    active.tree,
                    idle_since,
                ))
            })
            .collect()
    }

    /// Parks one due lane under its activation claim, after checking that no
    /// Turn or activation has used it since it was found due, and answers
    /// whether its runtime was released.
    fn park_idle_lane(
        &self,
        index: usize,
        owner: &RuntimeOwner,
        idle_since: Instant,
        deadline: Instant,
    ) -> bool {
        let lane = &self.lanes[index];
        if lane
            .activating
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let _claim = ActivationClaim { lane };
        let unchanged = lane
            .scheduler
            .active()
            .is_some_and(|active| &active.tree == owner && active.idle_since == Some(idle_since));
        if !unchanged || !self.parks_resumably(index) {
            return false;
        }
        self.lane_at(index).park_or_reclaim(deadline);
        let released = !lane.has_recovery()
            && lane
                .scheduler
                .active()
                .is_none_or(|active| &active.tree != owner);
        if !released {
            tracing::warn!("an idle Chat's runtime park did not settle");
        }
        released
    }
}

#[cfg(test)]
#[path = "idle_release_tests.rs"]
mod tests;
