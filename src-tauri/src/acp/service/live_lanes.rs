//! Bounded concurrent live runtimes (AP-B B2).
//!
//! Each lane is one complete copy of the formerly singleton live-runtime
//! authority: one runtime slot, one one-tree scheduler, its session
//! coordinator, and its single retained recovery. Every invariant the
//! single-runtime code proved still holds inside a lane; this module only
//! routes an exact runtime owner to the lane that holds it and picks a free
//! lane for a new activation. The lane count is the frozen
//! `operational_live_trees` limit, so total live harness trees stay bounded.

use std::{
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use crate::acp::{
    limits::LIMITS,
    runtime::{JournalHandle, ProjectionPublisher, RuntimeHost, SessionCoordinator},
    scheduler::{RuntimeOwner, Scheduler},
};

use super::{
    AcpService, CodexSessionError, RuntimeSlot, codex_live::PendingCodexRecovery,
    runtime_slot::RuntimeSlotError,
};

/// Number of concurrently live harness runtimes Sage admits.
pub(crate) const LIVE_RUNTIME_LANES: usize = LIMITS.process.operational_live_trees as usize;

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
}

/// One lane viewed through the service. Field names deliberately match the
/// former singleton `AcpService` fields, so lane-scoped runtime code reads
/// `self.runtime`, `self.scheduler`, `self.sessions` and `self.codex_recovery`
/// exactly as before while everything else dereferences to the service.
pub(super) struct Lane<'a> {
    service: &'a AcpService,
    pub(super) runtime: Arc<RuntimeSlot>,
    pub(super) scheduler: Scheduler,
    pub(super) sessions: SessionCoordinator,
    pub(super) codex_recovery: RecoveryCell,
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

impl AcpService {
    pub(super) fn lane_at(&self, index: usize) -> Lane<'_> {
        let lane = &self.lanes[index];
        Lane {
            service: self,
            runtime: Arc::clone(&lane.runtime),
            scheduler: lane.scheduler.clone(),
            sessions: lane.sessions.clone(),
            codex_recovery: Arc::clone(&lane.codex_recovery),
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
    /// (which activation parks). Busy lanes are never chosen.
    pub(super) fn claim_activation_lane(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<(Lane<'_>, ActivationClaim<'_>), CodexSessionError> {
        let candidates = self.activation_candidates(owner);
        for index in candidates {
            let lane = &self.lanes[index];
            if lane
                .activating
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                lane.last_activation.store(
                    crate::persistence::now_millis().max(0) as u64,
                    Ordering::Release,
                );
                return Ok((self.lane_at(index), ActivationClaim { lane }));
            }
        }
        Err(CodexSessionError::Capacity)
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
            .filter(|(index, lane)| {
                lane.scheduler.active().is_some_and(|active| {
                    !lane.has_recovery()
                        && self.can_resume(&active.tree)
                        && self.lane_at(*index).can_park_idle_runtime()
                })
            })
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

    fn can_resume(&self, owner: &RuntimeOwner) -> bool {
        self.database.connection().query_row(
            "SELECT capabilities_json FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 AND id=?3 AND external_session_id IS NOT NULL",
            rusqlite::params![owner.project_id.as_str(), owner.chat_id.as_str(), owner.session_id.as_str()],
            |row| row.get::<_, String>(0),
        ).ok().and_then(|json| serde_json::from_str::<crate::acp::intent::CapabilitySnapshotV1>(&json).ok())
            .is_some_and(|capabilities| capabilities.resume_session)
    }
}

#[cfg(test)]
#[path = "live_lanes_tests.rs"]
mod tests;
