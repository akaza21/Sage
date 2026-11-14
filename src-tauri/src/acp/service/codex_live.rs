use std::{
    sync::{Arc, Weak, atomic::AtomicU64},
    time::{Duration, Instant},
};

use crate::acp::{
    bindings::runtime::NativeRuntime,
    intent::IntentSnapshot,
    limits::LIMITS,
    process::{PlatformSupervisor, StdinEndpoint, TreeCapacityPool, TreeControl, TreeExit},
    resource_mapper::NativeResourceMapper,
    runtime::{
        ActivationProcessDriver, ActivationPump, ActivationStartFailure,
        ActivationZeroProcessHandoff, CoordinatorActivationJournal, CoordinatorShutdownHandoff,
        DriverClock, DriverMailbox, FirstTurnWorkerContext, LiveRunnerExit, LiveRunnerHandle,
        LiveRuntimeEngine, ProcessLivePort, ProjectIntentAuthority, RuntimeHost, SendCoordinator,
        SessionTreeLease, SystemDriverClock, ZeroProcessProof,
    },
    scheduler::RuntimeOwner,
    store::{
        AbortActivationCommand, ActivateSessionCommand, ActivationAbortCategory, PrepareTurnCommand,
    },
};

#[cfg(test)]
use super::AcpService;
use super::{CodexSessionError, codex_operation::ActiveCodexOperation, live_lanes::Lane};

struct DatabaseAuthority(crate::persistence::Database);

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
}
fn hex_digest(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

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
        crate::acp::runtime::HostError::Intent => CodexSessionError::Intent,
        crate::acp::runtime::HostError::SteerOutcomeUnknown => {
            CodexSessionError::SteerOutcomeUnknown
        }
        crate::acp::runtime::HostError::SignInRequired => CodexSessionError::SignInRequired,
        _ => CodexSessionError::Recovery,
    }
}
