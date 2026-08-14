use super::*;

impl AcpService {
    pub(crate) fn send_codex_turn(
        &self,
        command: PrepareTurnCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        let host = self.runtime.for_owner(&owner).map_err(map_slot_error)?;
        match host.send(crate::acp::runtime::HostPromptIntent { command }) {
            Ok(outcome) => Ok(outcome),
            Err(error) => Err(self.retain_live_host_error(owner, host, error)),
        }
    }

    pub(crate) fn cancel_codex_turn(
        &self,
        intent: crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let owner = intent.owner.clone();
        let host = self.runtime.for_owner(&owner).map_err(map_slot_error)?;
        match host.cancel(intent) {
            Ok(outcome) => Ok(outcome),
            Err(error) => Err(self.retain_live_host_error(owner, host, error)),
        }
    }

    pub(crate) fn close_codex_session(
        &self,
        command: crate::acp::store::CloseSessionCommand,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        let intent = crate::acp::runtime::HostCloseIntent { command };
        let host = match self.runtime.for_owner(&owner) {
            Ok(host) => host,
            Err(crate::acp::service::runtime_slot::RuntimeSlotError::Recovery) => {
                return self.recover_live_close(owner, intent);
            }
            Err(error) => return Err(map_slot_error(error)),
        };
        let outcome = match host.close(intent) {
            Ok(outcome) => outcome,
            Err(error) => return Err(self.retain_live_host_error(owner, host, error)),
        };
        if let Err(error) = host.shutdown() {
            return Err(self.retain_live_host_error(owner, host, error));
        }
        self.runtime
            .release(&owner, &host)
            .map_err(map_slot_error)?;
        Ok(outcome)
    }

    fn recover_live_close(
        &self,
        owner: RuntimeOwner,
        intent: crate::acp::runtime::HostCloseIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let Some(host) = self.take_live_host_recovery(&owner) else {
            return Err(CodexSessionError::Recovery);
        };
        let outcome = match host.recover_close(intent) {
            Ok(outcome) => outcome,
            Err(error) => return Err(self.retain_live_host_error(owner, host, error)),
        };
        if let Err(error) = host.shutdown() {
            return Err(self.retain_live_host_error(owner, host, error));
        }
        self.runtime
            .release_recovered(&owner, &host)
            .map_err(map_slot_error)?;
        Ok(outcome)
    }

    fn take_live_host_recovery(&self, owner: &RuntimeOwner) -> Option<Arc<RuntimeHost>> {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let recovery = retained.take()?;
        match recovery {
            PendingCodexRecovery::LiveHost {
                owner: retained_owner,
                host,
            } if &retained_owner == owner => Some(host),
            recovery => {
                *retained = Some(recovery);
                None
            }
        }
    }

    fn retain_live_host_error(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
        error: crate::acp::runtime::HostError,
    ) -> CodexSessionError {
        if requires_live_recovery(&error) {
            let _ = self.runtime.mark_recovery(&owner);
            self.retain_codex_recovery(PendingCodexRecovery::LiveHost { owner, host });
        }
        map_host_error(error)
    }
}

fn requires_live_recovery(error: &crate::acp::runtime::HostError) -> bool {
    matches!(
        error,
        crate::acp::runtime::HostError::Unavailable
            | crate::acp::runtime::HostError::DeliveryUnknown
            | crate::acp::runtime::HostError::RetryExhausted
            | crate::acp::runtime::HostError::RecoveryRequired
            | crate::acp::runtime::HostError::Deadline
            | crate::acp::runtime::HostError::Closed
    )
}
