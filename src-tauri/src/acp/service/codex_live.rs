use std::{
    sync::{Arc, Weak, atomic::AtomicU64},
    time::{Duration, Instant},
};

use crate::acp::{
    bindings::runtime::NativeRuntime,
    ipc::IntentSnapshot,
    process::{PlatformSupervisor, StdinEndpoint, TreeCapacityPool, TreeControl, TreeExit},
    resource_mapper::NativeResourceMapper,
    runtime::{
        ActivationProcessDriver, ActivationPump, ActivationStartFailure,
        ActivationZeroProcessHandoff, CoordinatorActivationJournal, CoordinatorShutdownHandoff,
        DriverClock, DriverMailbox, FirstTurnWorkerContext, LiveRunnerExit, LiveRunnerHandle,
        LiveRuntimeEngine, ProcessLivePort, ProjectIntentAuthority, RuntimeHost, SendCoordinator,
        SessionTreeLease, SystemDriverClock, ZeroProcessProof,
    },
    store::{
        AbortActivationCommand, ActivateSessionCommand, ActivationAbortCategory, PrepareTurnCommand,
    },
};
use sage_acp_protocol::scheduler::RuntimeOwner;
use sage_core::limits::LIMITS;

#[cfg(test)]
use super::AcpService;
use super::{CodexSessionError, codex_operation::ActiveCodexOperation, live_lanes::Lane};

struct DatabaseAuthority(
    sage_db::database::Database,
    Option<super::HarnessTurnAccess>,
);

#[derive(Clone)]
pub(super) struct ActivationAbortIntent {
    owner: RuntimeOwner,
    binding_digest: String,
    artifact_digest: String,
}

type CodexActivationDriver = ActivationProcessDriver<
    NativeRuntime,
    CoordinatorActivationJournal,
    NativeResourceMapper,
    StdinEndpoint,
    TreeControl,
>;

mod assembly;
mod cleanup;
mod live_cancel;
mod live_commands;
mod live_recovery;
mod live_send;
mod native_end;
mod projection_recovery;
mod recovery;
mod settle;
mod steer;

#[cfg(all(test, windows))]
mod journey_tests;
#[cfg(test)]
mod send_race_tests;

pub(super) use recovery::PendingCodexRecovery;

impl ActivationAbortIntent {
    fn from_prepared(prepared: &super::session::PreparedActivation) -> Self {
        Self {
            owner: prepared.owner.clone(),
            binding_digest: hex_digest(&prepared.intent.binding_digest),
            artifact_digest: prepared.intent.artifact_digest.clone(),
        }
    }

    fn command(&self, category: ActivationAbortCategory) -> AbortActivationCommand {
        AbortActivationCommand {
            project_id: self.owner.project_id.clone(),
            chat_id: self.owner.chat_id.clone(),
            session_id: self.owner.session_id.clone(),
            runtime_epoch: self.owner.runtime_epoch,
            binding_digest: self.binding_digest.clone(),
            artifact_digest: self.artifact_digest.clone(),
            category,
            now_millis: sage_core::time::now_millis(),
        }
    }
}

impl ProjectIntentAuthority for DatabaseAuthority {
    fn current_snapshot(&self, command: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        super::load_intent_snapshot(
            &self.0,
            command.project_id.clone(),
            command.project_generation,
            command.chat_id.clone(),
        )
        .ok()
    }

    fn harness_turn_mode(&self, owner: &RuntimeOwner) -> Option<&'static str> {
        self.1.as_ref().and_then(|access| access(owner)?.1)
    }

    fn harness_turn_access(
        &self,
        owner: &RuntimeOwner,
    ) -> Option<sage_acp_protocol::access_mode::SessionAccessMode> {
        self.1
            .as_ref()
            .and_then(|access| access(owner))
            .map(|grant| grant.0)
    }
}
fn hex_digest(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Launch is part of an activation's window: verifying, scanning and loading
/// a large harness build on its first start after an update, or on a loaded
/// machine, can take most of a request's deadline, which then left too little
/// for the harness's own start. A launch that took longer than an instant
/// moves both deadlines on by the time it took, up to this bound.
const LAUNCH_ALLOWANCE: Duration = Duration::from_secs(30);

pub(super) fn deadlines() -> Result<(Instant, Instant), CodexSessionError> {
    let cleanup = Instant::now() + Duration::from_millis(LIMITS.deadlines_ms.ordinary_rpc);
    let reserve = Duration::from_millis(
        LIMITS.deadlines_ms.tree_termination.saturating_mul(2)
            + LIMITS.deadlines_ms.wal_convergence,
    );
    let operation = cleanup
        .checked_sub(reserve)
        .ok_or(CodexSessionError::Capacity)?;
    (operation > Instant::now())
        .then_some((operation, cleanup))
        .ok_or(CodexSessionError::Capacity)
}

/// The activation's deadlines once its launch took `launched_in`: a fast
/// launch keeps them, a slow one moves them on by its length, at most
/// [`LAUNCH_ALLOWANCE`].
fn after_launch(
    (operation, cleanup): (Instant, Instant),
    launched_in: Duration,
) -> (Instant, Instant) {
    let late = launched_in.min(LAUNCH_ALLOWANCE);
    (operation + late, cleanup + late)
}

fn observe_runner(
    slot: &Arc<super::RuntimeSlot>,
    owner: &RuntimeOwner,
    runner: &Arc<LiveRunnerHandle>,
    recovery: &Arc<std::sync::Mutex<Option<PendingCodexRecovery>>>,
    active: Arc<std::sync::Mutex<Option<ActiveCodexOperation>>>,
    files: Arc<std::sync::Mutex<crate::acp::attached_context::files::FileStages>>,
    #[cfg(any(debug_assertions, feature = "diagnostics"))] identity: Arc<
        super::runtime_identity::CodexRuntimeIdentityLedger,
    >,
) {
    let slot: Weak<super::RuntimeSlot> = Arc::downgrade(slot);
    let weak_runner = Arc::downgrade(runner);
    let recovery = Arc::downgrade(recovery);
    let owner = owner.clone();
    runner.observe_exit_once(move |exit| {
        files
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear_chat(owner.project_id.as_str(), owner.chat_id.as_str());
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        identity.clear_exact(&owner);
        // The runner invokes this observer only after its own finish path has
        // reached terminal process handling. Retire only this exact runtime
        // owner before releasing any active-operation capacity; a newer
        // Preview owner cannot match these branded owner fields.
        let preview_retired =
            crate::app_preview_commands::retire_preview_runtime_owner(&owner).is_ok();
        if exit == LiveRunnerExit::Failed || !preview_retired {
            let recovered = slot
                .upgrade()
                .is_some_and(|slot| slot.mark_recovery(&owner).is_ok());
            if recovered
                && let (Some(recovery), Some(runner)) = (recovery.upgrade(), weak_runner.upgrade())
            {
                let mut retained = recovery
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if retained.is_none() {
                    *retained = Some(PendingCodexRecovery::Runner {
                        owner: owner.clone(),
                        runner,
                    });
                }
            }
            if let Some(active) = active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                active.settle(Err(
                    crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
                ));
            }
        } else if let Some(active) = active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            active.settle(Ok(()));
        }
    });
}

#[cfg(test)]
pub(super) fn observe_runner_failure_for_test(
    slot: &Arc<super::RuntimeSlot>,
    owner: &RuntimeOwner,
    runner: &Arc<LiveRunnerHandle>,
    recovery: &Arc<std::sync::Mutex<Option<PendingCodexRecovery>>>,
    active: Arc<std::sync::Mutex<Option<ActiveCodexOperation>>>,
) {
    observe_runner(
        slot,
        owner,
        runner,
        recovery,
        active,
        Arc::new(std::sync::Mutex::new(Default::default())),
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        Arc::new(super::runtime_identity::CodexRuntimeIdentityLedger::new()),
    );
    runner.fail_for_test();
}

fn map_gate_error(error: crate::acp::bindings::codex::CodexDiscoveryRunError) -> CodexSessionError {
    match error {
        crate::acp::bindings::codex::CodexDiscoveryRunError::Busy => CodexSessionError::Capacity,
        _ => CodexSessionError::Recovery,
    }
}

fn map_slot_error(error: super::runtime_slot::RuntimeSlotError) -> CodexSessionError {
    match error {
        super::runtime_slot::RuntimeSlotError::Occupied => CodexSessionError::Capacity,
        super::runtime_slot::RuntimeSlotError::Foreign => CodexSessionError::Intent,
        super::runtime_slot::RuntimeSlotError::Stopping
        | super::runtime_slot::RuntimeSlotError::Recovery => CodexSessionError::Recovery,
    }
}

fn map_host_error(error: crate::acp::runtime::HostError) -> CodexSessionError {
    match error {
        crate::acp::runtime::HostError::Capacity => CodexSessionError::Capacity,
        crate::acp::runtime::HostError::RepositoryBusy(pause) => {
            CodexSessionError::RepositoryBusy(pause)
        }
        crate::acp::runtime::HostError::Intent => CodexSessionError::Intent,
        crate::acp::runtime::HostError::SteerOutcomeUnknown => {
            CodexSessionError::SteerOutcomeUnknown
        }
        crate::acp::runtime::HostError::SignInRequired => CodexSessionError::SignInRequired,
        _ => CodexSessionError::Recovery,
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;

    #[test]
    fn a_launch_moves_the_deadlines_on_by_its_length_up_to_the_allowance() {
        let before = deadlines().unwrap();
        let (operation, cleanup) = before;
        // A fast launch keeps the deadlines a harness start had before.
        assert_eq!(after_launch(before, Duration::ZERO), before);
        // A slow one gets its own time back, so the harness keeps its start window.
        let slow = Duration::from_secs(12);
        assert_eq!(
            after_launch(before, slow),
            (operation + slow, cleanup + slow)
        );
        // An overlong launch is bounded by the allowance.
        assert_eq!(
            after_launch(before, Duration::from_secs(600)),
            (operation + LAUNCH_ALLOWANCE, cleanup + LAUNCH_ALLOWANCE)
        );
    }
}
