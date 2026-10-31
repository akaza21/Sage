//!  a turn whose host failed closed is always settled, and its lane freed.
//!
//! A harness can refuse a prompt before writing it (for example Claude Code
//! reporting that sign-in is required). The runtime host then fails closed and
//! rejects every later command for that turn as stale, so neither the turn nor
//! the lane could ever settle. These paths converge that exact tree through
//! the existing zero-process recovery, which terminalizes the durable turn and
//! releases the lane's capacity.

use super::live_recovery::LiveHostRecoveryPhase;
use super::*;

impl Lane<'_> {
    /// Settles a prompt the harness refused before any byte was written.
    pub(super) fn settle_refused_prompt(&self, owner: RuntimeOwner, host: Arc<RuntimeHost>) {
        tracing::warn!(
            recovery_checkpoint = "codex_live.send.refused",
            runtime_epoch = owner.runtime_epoch.get(),
            "Harness refused the prompt; settling the turn and releasing the runtime"
        );
        self.retain_live_failure_recovery(owner, host, LiveHostRecoveryPhase::SendEffect);
        let recover = || {
            let _ = self.shutdown_codex_recovery(close_deadline());
        };
        // Hand this thread to Tokio's blocking pool while preserving synchronous
        // durable settlement; unrelated async commands can keep making progress.
        if tokio::runtime::Handle::try_current().is_ok_and(|handle| {
            handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
        }) {
            tokio::task::block_in_place(recover);
        } else {
            recover();
        }
    }

    pub(super) fn host_failed_closed(&self, host: &Arc<RuntimeHost>) -> bool {
        matches!(host.state(), Ok(crate::acp::runtime::HostState::Recovery))
    }

    /// Cancel on a host that can no longer address its turn converges the
    /// tree instead of reporting a stale identity.
    pub(super) fn settle_cancel_on_failed_host(
        &self,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
        host: Arc<RuntimeHost>,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        self.retain_live_failure_recovery(owner.clone(), host, LiveHostRecoveryPhase::CancelEffect);
        let _ = self.shutdown_codex_recovery(close_deadline());
        self.settled_cancel_outcome(&owner, &turn_id)
    }

    /// Cancel succeeds once the exact turn is durably terminal, whichever
    /// path terminalized it.
    pub(super) fn settled_cancel_outcome(
        &self,
        owner: &RuntimeOwner,
        turn_id: &crate::acp::contract::SageTurnId,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        if self.turn_is_terminal(owner, turn_id) {
            Ok(crate::acp::runtime::HostOutcome::CancelQueued)
        } else {
            Err(CodexSessionError::Recovery)
        }
    }
}

fn close_deadline() -> Instant {
    Instant::now() + Duration::from_millis(LIMITS.deadlines_ms.explicit_close)
}
