//! Bounded concurrent live runtimes.
//!
//! Each lane owns one runtime slot, a one-tree scheduler, its session
//! coordinator, and one retained recovery. Routing selects the lane holding
//! an exact runtime owner or reserves a lane for a new activation. The
//! `operational_live_trees` limit bounds total live harness trees.

use std::{
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::acp::{
    runtime::{JournalHandle, ProjectionPublisher, RuntimeHost, SessionCoordinator},
    scheduler::{RuntimeOwner, Scheduler},
};
use sage_core::limits::LIMITS;

use super::{
    AcpService, CodexSessionError, RuntimeSlot, codex_live::PendingCodexRecovery,
    runtime_slot::RuntimeSlotError,
};

/// Number of concurrently live harness runtimes Sage admits.
pub(crate) const LIVE_RUNTIME_LANES: usize = LIMITS.process.operational_live_trees as usize;

/// The explicit-close deadline from now: the bound on one release or close.
pub(crate) fn explicit_close_deadline() -> Instant {
    Instant::now() + Duration::from_millis(LIMITS.deadlines_ms.explicit_close)
}

/// Why a Chat's live runtime was not released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatRuntimeRelease {
    /// A Turn is running; the user stops it first.
    TurnRunning,
    /// An activation or another release holds the lane; once it lets go, a
    /// retry may succeed.
    LaneBusy,
    /// The park did not settle before its deadline; retrying now does not
    /// help.
    Unsettled,
    /// The harness is still answering a session control (a fork, a usage
    /// read, a setting change); once it answers, a retry may succeed.
    ControlRunning,
}

impl ChatRuntimeRelease {
    /// The refusal shown to the user for `action` ("archiving", "deleting").
    pub(crate) fn message(self, action: &str) -> String {
        match self {
            Self::TurnRunning => format!("Stop this Chat's running turn before {action} it"),
            Self::LaneBusy | Self::Unsettled => {
                format!("This Chat's agent is still shutting down; try {action} it again")
            }
            Self::ControlRunning => {
                format!("This Chat's agent is still answering a request; try {action} it again")
            }
        }
    }
}

pub(super) type RecoveryCell = Arc<Mutex<Option<PendingCodexRecovery>>>;

pub(super) struct LiveLane {
    runtime: Arc<RuntimeSlot>,
    scheduler: Scheduler,
    sessions: SessionCoordinator,
    codex_recovery: RecoveryCell,
    activating: AtomicBool,
    last_activation: AtomicU64,
}

impl LiveLane {
    pub(super) fn new(journal: &JournalHandle, publisher: &Arc<dyn ProjectionPublisher>) -> Self {
        let scheduler = Scheduler::new();
        Self {
            runtime: Arc::new(RuntimeSlot::new()),
            sessions: SessionCoordinator::new(
                journal.clone(),
                scheduler.clone(),
                Arc::clone(publisher),
            ),
            scheduler,
            codex_recovery: Arc::new(Mutex::new(None)),
            activating: AtomicBool::new(false),
            last_activation: AtomicU64::new(0),
        }
    }

    fn recovery_owner(&self) -> Option<RuntimeOwner> {
        self.codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(PendingCodexRecovery::owner)
    }

    fn has_recovery(&self) -> bool {
        self.codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    /// True when this lane retains any authority for the exact owner.
    fn holds(&self, owner: &RuntimeOwner) -> bool {
        self.runtime.owner().as_ref() == Some(owner)
            || self
                .scheduler
                .active()
                .is_some_and(|active| &active.tree == owner)
            || self.recovery_owner().as_ref() == Some(owner)
    }

    /// True when this lane retains any authority for the same session at any
    /// runtime epoch. A re-activation must land where an older epoch's
    /// recovery can be reclaimed first.
    fn holds_session(&self, owner: &RuntimeOwner) -> bool {
        let same = |candidate: &RuntimeOwner| {
            candidate.project_id == owner.project_id
                && candidate.chat_id == owner.chat_id
                && candidate.session_id == owner.session_id
        };
        self.runtime.owner().as_ref().is_some_and(same)
            || self
                .scheduler
                .active()
                .is_some_and(|active| same(&active.tree))
            || self.recovery_owner().as_ref().is_some_and(same)
    }

    fn is_free(&self) -> bool {
        self.runtime.is_empty() && self.scheduler.active().is_none() && !self.has_recovery()
    }

    /// An idle runtime that would make room for another Chat, but whose
    /// harness is answering a session control.
    fn answering_while_idle(&self) -> bool {
        !self.has_recovery()
            && self.scheduler.active().is_some_and(|active| {
                !active.turn_reserved
                    && self
                        .runtime
                        .for_owner(&active.tree)
                        .is_ok_and(|host| !host.controls_settled())
            })
    }
}

/// Lane-specific runtime authority with shared service access through `Deref`.
pub(super) struct Lane<'a> {
    service: &'a AcpService,
    pub(super) runtime: Arc<RuntimeSlot>,
    pub(super) scheduler: Scheduler,
    pub(super) sessions: SessionCoordinator,
    pub(super) codex_recovery: RecoveryCell,
    /// The idle park claimed on another Chat's runtime when this lane was
    /// chosen to evict it; its park takes it, and dropping it lets that
    /// runtime's session controls through again.
    pub(super) eviction: Mutex<Option<crate::acp::runtime::IdleParkClaim>>,
}

impl Deref for Lane<'_> {
    type Target = AcpService;

    fn deref(&self) -> &AcpService {
        self.service
    }
}

/// Releases a lane's activation reservation however activation ends.
pub(super) struct ActivationClaim<'a> {
    lane: &'a LiveLane,
}

impl Drop for ActivationClaim<'_> {
    fn drop(&mut self) {
        self.lane.activating.store(false, Ordering::Release);
    }
}

impl Lane<'_> {
    /// Parks this lane's idle runtime. A park that did not converge retains
    /// its zero-process recovery, driven now as an activation's reclaim would.
    fn park_or_reclaim(&self, deadline: Instant) {
        if !matches!(self.park_idle_runtime(deadline), Ok(true))
            && self.has_replacement_park_recovery()
        {
            let _ = self.shutdown_codex_recovery(deadline);
        }
    }
}

impl AcpService {
    pub(super) fn lane_at(&self, index: usize) -> Lane<'_> {
        let lane = &self.lanes[index];
        Lane {
            service: self,
            runtime: Arc::clone(&lane.runtime),
            scheduler: lane.scheduler.clone(),
            sessions: lane.sessions.clone(),
            codex_recovery: Arc::clone(&lane.codex_recovery),
            eviction: Mutex::new(None),
        }
    }

    pub(super) fn lanes(&self) -> impl Iterator<Item = Lane<'_>> {
        (0..self.lanes.len()).map(|index| self.lane_at(index))
    }

    /// The lane retaining any authority for this exact owner.
    pub(super) fn lane_for(&self, owner: &RuntimeOwner) -> Option<Lane<'_>> {
        self.lanes
            .iter()
            .position(|lane| lane.holds(owner))
            .map(|index| self.lane_at(index))
    }

    pub(super) fn host_for(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<Arc<RuntimeHost>, RuntimeSlotError> {
        self.lane_for(owner)
            .ok_or(RuntimeSlotError::Foreign)?
            .runtime
            .for_owner(owner)
    }

    pub(super) fn live_runtime_count(&self) -> usize {
        self.lanes
            .iter()
            .filter(|lane| lane.runtime.is_active())
            .count()
    }

    /// Picks the lane for a new activation and reserves it until the claim is
    /// dropped, so concurrent activations never race for one lane. Order:
    /// a lane already holding this session, a free lane, a lane with
    /// resumable idle runtime, then a lane retaining recovery
    /// (which activation parks). Busy lanes are never chosen. Choosing a lane
    /// that holds another Chat's idle runtime claims that runtime's idle park
    /// at once; one whose harness is answering a session control is passed
    /// over for the next candidate.
    pub(super) fn claim_activation_lane(
        &self,
        owner: &RuntimeOwner,
        deadline: Instant,
    ) -> Result<(Lane<'_>, ActivationClaim<'_>), CodexSessionError> {
        // Reclaim a failed replacement before evicting another idle chat.
        // A same-session activation must keep its original revision and lane.
        if !self.lanes.iter().any(|lane| lane.holds_session(owner)) {
            self.reclaim_replacement_parks(deadline);
        }
        let candidates = self.activation_candidates(owner);
        for index in candidates {
            let lane = &self.lanes[index];
            if lane
                .activating
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                continue;
            }
            let claim = ActivationClaim { lane };
            let view = self.lane_at(index);
            let evicted = lane
                .scheduler
                .active()
                .filter(|_| !lane.holds_session(owner) && !lane.has_recovery())
                .and_then(|active| lane.runtime.for_owner(&active.tree).ok());
            if let Some(host) = evicted {
                let Some(park) = host.claim_idle_park() else {
                    continue;
                };
                *view
                    .eviction
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(park);
            }
            lane.last_activation.store(
                sage_core::time::now_millis().max(0) as u64,
                Ordering::Release,
            );
            return Ok((view, claim));
        }
        if self.lanes.iter().any(|lane| lane.answering_while_idle()) {
            return Err(CodexSessionError::CapacityAnswering);
        }
        Err(CodexSessionError::Capacity)
    }

    /// Parks the Chat's live runtime through the explicit-close park an
    /// evicted idle lane takes, so an archived or deleted Chat holds no
    /// harness process; a later restore resumes the session. A lane retaining
    /// recovery for the Chat is driven first. A reserved Turn is refused.
    /// `still_wanted` is asked once the lane is claimed, so a Chat restored
    /// meanwhile keeps its runtime; `deadline` bounds the whole release.
    pub(crate) fn release_chat_runtime(
        &self,
        project: &crate::acp::contract::SageProjectId,
        chat: &crate::acp::contract::SageChatId,
        deadline: Instant,
        still_wanted: impl Fn() -> bool,
    ) -> Result<(), ChatRuntimeRelease> {
        let same = |owner: &RuntimeOwner| &owner.project_id == project && &owner.chat_id == chat;
        for (index, lane) in self.lanes.iter().enumerate() {
            let active = lane.scheduler.active().filter(|active| same(&active.tree));
            let recovering = lane.recovery_owner().as_ref().is_some_and(same);
            if active.is_none() && !recovering {
                continue;
            }
            if active.is_some_and(|active| active.turn_reserved) {
                return Err(ChatRuntimeRelease::TurnRunning);
            }
            // An activation in flight on this lane owns it until it settles.
            if lane
                .activating
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                #[cfg(test)]
                self.observe_lane_busy_for_test(lane);
                return Err(ChatRuntimeRelease::LaneBusy);
            }
            let _claim = ActivationClaim { lane };
            if !still_wanted() {
                return Ok(());
            }
            let view = self.lane_at(index);
            if recovering {
                let _ = view.shutdown_codex_recovery(deadline);
            }
            if lane
                .scheduler
                .active()
                .is_some_and(|active| same(&active.tree))
            {
                view.park_or_reclaim(deadline);
            }
            let held = lane.scheduler.active().filter(|active| same(&active.tree));
            // A prompt that won the idle-drain race keeps its runtime running.
            if held.as_ref().is_some_and(|active| active.turn_reserved) {
                return Err(ChatRuntimeRelease::TurnRunning);
            }
            // So does a session control the harness was answering: a park
            // that leaves a parkable runtime untouched was refused for one,
            // even if the harness has answered it since, and a retry parks it.
            if !lane.has_recovery()
                && held.as_ref().is_some_and(|active| {
                    view.runtime
                        .for_owner(&active.tree)
                        .is_ok_and(|host| host.has_shutdown_recovery())
                })
            {
                #[cfg(test)]
                self.observe_control_running_for_test();
                return Err(ChatRuntimeRelease::ControlRunning);
            }
            if held.is_some() || lane.recovery_owner().as_ref().is_some_and(same) {
                return Err(ChatRuntimeRelease::Unsettled);
            }
        }
        Ok(())
    }

    fn reclaim_replacement_parks(&self, deadline: Instant) {
        let deadline = deadline.min(explicit_close_deadline());
        for (index, lane) in self.lanes.iter().enumerate() {
            if Instant::now() >= deadline {
                break;
            }
            if !lane.has_recovery()
                || lane
                    .activating
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
            {
                continue;
            }
            let _claim = ActivationClaim { lane };
            let lane = self.lane_at(index);
            if lane.has_replacement_park_recovery() {
                // Failure keeps recovery authority and must not starve other lanes.
                let _ = lane.shutdown_codex_recovery(deadline);
            }
        }
    }

    fn activation_candidates(&self, owner: &RuntimeOwner) -> Vec<usize> {
        let mut order = Vec::with_capacity(self.lanes.len());
        let push = |index: usize, order: &mut Vec<usize>| {
            if !order.contains(&index) {
                order.push(index);
            }
        };
        if let Some(index) = self.lanes.iter().position(|lane| lane.holds_session(owner)) {
            push(index, &mut order);
            // The session's own lane must reclaim its older epoch first; it
            // is the only lane where a second tree for one session could
            // otherwise appear.
            return order;
        }
        for (index, lane) in self.lanes.iter().enumerate() {
            if lane.is_free() {
                push(index, &mut order);
            }
        }
        let mut idle: Vec<(u64, usize)> = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(index, _)| self.parks_resumably(*index))
            .map(|(index, lane)| (lane.last_activation.load(Ordering::Acquire), index))
            .collect();
        idle.sort_unstable();
        for (_, index) in idle {
            push(index, &mut order);
        }
        for (index, lane) in self.lanes.iter().enumerate() {
            if lane.has_recovery() {
                push(index, &mut order);
            }
        }
        order
    }

    /// An idle runtime a park releases and the next activation resumes;
    /// eviction and idle release park only these.
    fn parks_resumably(&self, index: usize) -> bool {
        let lane = &self.lanes[index];
        lane.scheduler.active().is_some_and(|active| {
            !lane.has_recovery()
                && self.can_resume(&active.tree)
                && self.lane_at(index).can_park_idle_runtime()
        })
    }

    fn can_resume(&self, owner: &RuntimeOwner) -> bool {
        self.database.connection().query_row(
            "SELECT capabilities_json FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 AND id=?3 AND external_session_id IS NOT NULL",
            rusqlite::params![owner.project_id.as_str(), owner.chat_id.as_str(), owner.session_id.as_str()],
            |row| row.get::<_, String>(0),
        ).ok().and_then(|json| serde_json::from_str::<crate::acp::intent::CapabilitySnapshotV1>(&json).ok())
            .is_some_and(|capabilities| capabilities.resume_session)
    }
}

mod idle_release;
pub(crate) use idle_release::IdleRelease;
mod release_retry;
mod turn_undo;

#[cfg(test)]
#[path = "live_lanes_tests.rs"]
mod tests;
