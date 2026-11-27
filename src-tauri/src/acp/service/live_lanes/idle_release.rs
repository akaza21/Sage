use std::{
    collections::{HashMap, HashSet},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use sage_acp_protocol::{contract::SageChatId, scheduler::RuntimeOwner};

use super::{AcpService, ActivationClaim, explicit_close_deadline};
use crate::acp::service::background_terminals::TerminalsError;

const HIDDEN_IDLE: Duration = Duration::from_secs(2 * 60);
/// The Chat on screen keeps its runtime this long after its last turn, so a
/// follow-up within a few minutes needs no restart; after that the harness
/// process (often hundreds of MB) is released and the next prompt starts it.
const SHOWN_IDLE: Duration = Duration::from_secs(5 * 60);

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
    pub(crate) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        drop(self.sweep.lock().unwrap_or_else(PoisonError::into_inner));
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }
}

impl AcpService {
    pub(crate) const IDLE_SWEEP_INTERVAL: Duration = Duration::from_secs(15);

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

    /// Answers how many were parked, or None once sweeping has stopped.
    pub(crate) fn release_idle_runtimes(&self, now: Instant) -> Option<usize> {
        let release = &self.idle_release;
        let _sweep = release.sweep.lock().unwrap_or_else(PoisonError::into_inner);
        if release.is_stopped() {
            return None;
        }
        let mut parked = 0;
        for (index, owner, idle_since) in self.idle_runtimes_due(now) {
            if release.is_stopped() {
                break;
            }
            // Each park has the whole close budget, as an eviction does.
            let deadline = explicit_close_deadline();
            parked += usize::from(self.park_idle_lane(index, &owner, idle_since, now, deadline));
        }
        Some(parked)
    }

    pub(in crate::acp::service) fn idle_runtimes_due(
        &self,
        now: Instant,
    ) -> Vec<(usize, RuntimeOwner, Instant)> {
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

    pub(in crate::acp::service) fn park_idle_lane(
        &self,
        index: usize,
        owner: &RuntimeOwner,
        idle_since: Instant,
        now: Instant,
        deadline: Instant,
    ) -> bool {
        // Asked before the lane is claimed, so a prompt to this Chat never
        // finds its lane busy while the harness answers. New work needs a
        // turn, which changes `idle_since` and fails the check below.
        if self.runs_background_work(owner) {
            return false;
        }
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
        // The Chat may have come back into view while earlier lanes parked.
        let still_due = || {
            self.idle_release
                .view
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .due(&owner.chat_id, idle_since, now)
        };
        if !unchanged || !still_due() || !self.parks_resumably(index) {
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

    /// Background work the runtime still runs after its turn keeps it: work
    /// the session surface lists, or terminals the harness reports. The sweep
    /// asks again each time, so the runtime is released once the work ends.
    fn runs_background_work(&self, owner: &RuntimeOwner) -> bool {
        let listed = crate::acp::store::session_surface::runs_background_work(
            &self.database.connection(),
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
        );
        match listed {
            Ok(false) => {}
            Ok(true) => return true,
            Err(_) => {
                // A surface that cannot be read may hide work.
                tracing::warn!("an idle Chat's session surface is unreadable; keeping its runtime");
                return true;
            }
        }
        if !self
            .session_capabilities(owner)
            .is_some_and(|capabilities| capabilities.native_background_terminals)
        {
            return false;
        }
        match self.idle_terminals(owner) {
            Ok(terminals) => !terminals.is_empty(),
            // The runtime was busy, or the answer is late or lost.
            Err(TerminalsError::RecoveryRequired) => true,
            Err(TerminalsError::Rejected | TerminalsError::Stale) => false,
        }
    }
}

#[cfg(test)]
#[path = "idle_release_tests.rs"]
mod tests;
