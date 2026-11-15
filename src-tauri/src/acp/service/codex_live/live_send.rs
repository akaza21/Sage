use super::live_recovery::LiveHostRecoveryPhase;
use super::*;

impl Lane<'_> {
    pub(crate) fn send_session_turn(
        &self,
        intent: crate::acp::runtime::HostPromptIntent,
    ) -> Result<
        crate::acp::runtime::HostOutcome,
        crate::acp::prompt_send::PromptSendError<CodexSessionError>,
    > {
        let started = Instant::now();
        let owner = RuntimeOwner {
            project_id: intent.command.project_id.clone(),
            chat_id: intent.command.chat_id.clone(),
            session_id: intent.command.session_id.clone(),
            runtime_epoch: intent.command.runtime_epoch,
        };
        let host = self
            .runtime
            .for_owner(&owner)
            .inspect_err(|error| {
                crate::acp::runtime::prompt_diagnostics::failed(
                    "service.runtime_owner",
                    started,
                    error,
                    owner.runtime_epoch,
                );
            })
            .map_err(map_slot_error)
            .map_err(crate::acp::prompt_send::PromptSendError::not_started)?;
        match host.send(intent) {
            Ok(outcome) => Ok(outcome),
            Err(failure) if failure.error == crate::acp::runtime::HostError::SignInRequired => {
                // Refusal happened before the prompt was written.
                self.settle_refused_prompt(owner, host);
                Err(crate::acp::prompt_send::PromptSendError::unknown(
                    CodexSessionError::SignInRequired,
                ))
            }
            Err(failure) => {
                crate::acp::runtime::prompt_diagnostics::failed(
                    "service.host_send",
                    started,
                    &failure.error,
                    (
                        "slot_error",
                        self.runtime.for_owner(&owner).err(),
                        "lifecycle",
                        host.lifecycle.load(std::sync::atomic::Ordering::Acquire),
                    ),
                );
                Err(failure.map(|error| {
                    self.retain_live_host_error(
                        owner,
                        host,
                        LiveHostRecoveryPhase::SendEffect,
                        error,
                    )
                }))
            }
        }
    }
}
