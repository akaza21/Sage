//! A harness can refuse a prompt before writing it (for example Claude Code
//! reporting that sign-in is required). The runtime host then fails closed and
//! rejects every later command for that turn as stale.

use super::live_recovery::LiveHostRecoveryPhase;
use super::*;

impl Lane<'_> {
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
        sage_core::blocking::in_place(recover);
    }

    pub(super) fn host_failed_closed(&self, host: &Arc<RuntimeHost>) -> bool {
        matches!(host.state(), Ok(crate::acp::runtime::HostState::Recovery))
    }

    pub(super) fn settle_cancel_on_failed_host(
        &self,
        owner: RuntimeOwner,
        turn_id: sage_acp_protocol::contract::SageTurnId,
        host: Arc<RuntimeHost>,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        self.retain_live_failure_recovery(owner.clone(), host, LiveHostRecoveryPhase::CancelEffect);
        let _ = self.shutdown_codex_recovery(close_deadline());
        self.settled_cancel_outcome(&owner, &turn_id)
    }

    pub(super) fn settled_cancel_outcome(
        &self,
        owner: &RuntimeOwner,
        turn_id: &sage_acp_protocol::contract::SageTurnId,
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
