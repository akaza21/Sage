use super::live_recovery::LiveHostRecoveryPhase;
use super::*;

impl AcpService {
    #[cfg(test)]
    pub(crate) fn send_codex_turn(
        &self,
        command: PrepareTurnCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        self.send_session_turn(crate::acp::runtime::HostPromptIntent {
            command,
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .map_err(|failure| failure.error)
    }

    pub(crate) fn send_session_turn(
        &self,
        intent: crate::acp::runtime::HostPromptIntent,
    ) -> Result<
        crate::acp::runtime::HostOutcome,
        crate::acp::service::PromptSendError<CodexSessionError>,
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
            .map_err(crate::acp::service::PromptSendError::not_started)?;
        match host.send(intent) {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                crate::acp::runtime::prompt_diagnostics::failed(
                    "service.host_send",
                    started,
                    &error,
                    (
                        "slot_error",
                        self.runtime.for_owner(&owner).err(),
                        "lifecycle",
                        host.lifecycle.load(std::sync::atomic::Ordering::Acquire),
                    ),
                );
                Err(crate::acp::service::PromptSendError::unknown(
                    self.retain_live_host_error(
                        owner,
                        host,
                        LiveHostRecoveryPhase::SendEffect,
                        error,
                    ),
                ))
            }
        }
    }
}
