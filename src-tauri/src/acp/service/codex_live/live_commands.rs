use super::live_recovery::{ClosedShutdownAuthority, LiveHostRecovery, LiveHostRecoveryPhase};
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
            Err(error) => Err(self.retain_live_host_error(
                owner,
                host,
                LiveHostRecoveryPhase::SendEffect,
                error,
            )),
        }
    }

    pub(crate) fn cancel_codex_turn(
        &self,
        intent: crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let owner = intent.owner.clone();
        let turn_id = intent.turn_id.clone();
        let host = self.runtime.for_owner(&owner).map_err(map_slot_error)?;
        match host.cancel(intent) {
            Ok(outcome) => self.finish_cancelled_shutdown(owner, turn_id, host, outcome),
            Err(error) => Err(self.retain_live_host_error(
                owner,
                host,
                LiveHostRecoveryPhase::CancelEffect,
                error,
            )),
        }
    }

    fn finish_cancelled_shutdown(
        &self,
        owner: RuntimeOwner,
        turn_id: crate::acp::contract::SageTurnId,
        host: Arc<RuntimeHost>,
        outcome: crate::acp::runtime::HostOutcome,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let deadline = Instant::now()
            + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        #[cfg(test)]
        self.record_cancel_stage("durable-wait");
        while !self.cancel_is_durable(&owner, &turn_id) {
            if Instant::now() >= deadline {
                #[cfg(test)]
                self.record_cancel_stage("durable-timeout");
                let _ = self.runtime.mark_recovery(&owner);
                self.retain_live_recovery(LiveHostRecovery {
                    owner,
                    host,
                    phase: LiveHostRecoveryPhase::CancelEffect,
                });
                return Err(CodexSessionError::Recovery);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        #[cfg(test)]
        self.record_cancel_stage("converge");
        match self.converge_closed_shutdown(
            &owner,
            &host,
            ClosedShutdownAuthority::Host,
            false,
            deadline,
        ) {
            Ok(()) => {
                #[cfg(test)]
                self.record_cancel_stage("park");
                self.park_after_cancel_zero(&owner).map(|()| {
                    #[cfg(test)]
                    self.record_cancel_stage("ok");
                    outcome
                })
            }
            Err(_) => {
                #[cfg(test)]
                self.record_cancel_stage("converge");
                let _ = self.runtime.mark_recovery(&owner);
                self.retain_live_recovery(LiveHostRecovery {
                    owner,
                    host,
                    phase: LiveHostRecoveryPhase::CancelEffect,
                });
                Err(CodexSessionError::Recovery)
            }
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
        let outcome = match host.close(intent.clone()) {
            Ok(outcome) => outcome,
            Err(error) => {
                return Err(self.retain_live_host_error(
                    owner,
                    host,
                    LiveHostRecoveryPhase::CloseEffect { intent },
                    error,
                ));
            }
        };
        self.finish_closed_shutdown(owner, host, intent, outcome, false)
    }

    fn recover_live_close(
        &self,
        owner: RuntimeOwner,
        intent: crate::acp::runtime::HostCloseIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let Some(recovery) = self.take_live_host_recovery(&owner) else {
            if self.has_runtime_recovery_for(&owner) {
                let deadline = Instant::now()
                    + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
                let _ = self.shutdown_codex_recovery(deadline);
            }
            return Err(CodexSessionError::Recovery);
        };
        if recovery.owner != owner {
            self.retain_codex_recovery(PendingCodexRecovery::LiveHost(recovery));
            return Err(CodexSessionError::Intent);
        }
        let LiveHostRecovery { host, phase, .. } = recovery;
        match phase {
            phase @ (LiveHostRecoveryPhase::SendEffect | LiveHostRecoveryPhase::CancelEffect) => {
                let deadline = Instant::now()
                    + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
                if let Some(recovery) = self
                    .shutdown_live_host_recovery(LiveHostRecovery { owner, host, phase }, deadline)
                {
                    self.retain_codex_recovery(recovery);
                }
                Err(CodexSessionError::Recovery)
            }
            LiveHostRecoveryPhase::CloseEffect {
                intent: retained_intent,
            } => {
                if retained_intent != intent {
                    self.retain_live_recovery(LiveHostRecovery {
                        owner,
                        host,
                        phase: LiveHostRecoveryPhase::CloseEffect {
                            intent: retained_intent,
                        },
                    });
                    return Err(CodexSessionError::Intent);
                }
                match host.recover_close(intent.clone()) {
                    Ok(outcome) => self.finish_closed_shutdown(owner, host, intent, outcome, true),
                    Err(error) => {
                        self.retain_live_recovery(LiveHostRecovery {
                            owner,
                            host,
                            phase: LiveHostRecoveryPhase::CloseEffect { intent },
                        });
                        Err(map_host_error(error))
                    }
                }
            }
            LiveHostRecoveryPhase::Closed {
                intent: retained_intent,
                outcome,
                authority,
            } => {
                if retained_intent != intent {
                    self.retain_live_recovery(LiveHostRecovery {
                        owner,
                        host,
                        phase: LiveHostRecoveryPhase::Closed {
                            intent: retained_intent,
                            outcome,
                            authority,
                        },
                    });
                    return Err(CodexSessionError::Intent);
                }
                self.finish_closed_authority(owner, host, intent, outcome, authority, true)
            }
        }
    }

    fn finish_closed_shutdown(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
        intent: crate::acp::runtime::HostCloseIntent,
        outcome: crate::acp::runtime::HostOutcome,
        recovering: bool,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        self.finish_closed_authority(
            owner,
            host,
            intent,
            outcome,
            ClosedShutdownAuthority::Host,
            recovering,
        )
    }

    fn finish_closed_authority(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
        intent: crate::acp::runtime::HostCloseIntent,
        outcome: crate::acp::runtime::HostOutcome,
        authority: ClosedShutdownAuthority,
        recovering: bool,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let deadline = Instant::now()
            + Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        match self.converge_closed_shutdown(&owner, &host, authority, recovering, deadline) {
            Ok(()) => Ok(outcome),
            Err(authority) => {
                let _ = self.runtime.mark_recovery(&owner);
                self.retain_live_recovery(LiveHostRecovery {
                    owner,
                    host,
                    phase: LiveHostRecoveryPhase::Closed {
                        intent,
                        outcome,
                        authority,
                    },
                });
                Err(CodexSessionError::Recovery)
            }
        }
    }

    pub(super) fn converge_closed_shutdown(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
        mut authority: ClosedShutdownAuthority,
        recovering: bool,
        deadline: Instant,
    ) -> Result<(), ClosedShutdownAuthority> {
        loop {
            authority = match authority {
                ClosedShutdownAuthority::Host => match host.shutdown_recovery_before(deadline) {
                    Ok(Some(token)) => ClosedShutdownAuthority::Token(token),
                    Ok(None) => ClosedShutdownAuthority::Release(None),
                    Err(_) => return Err(ClosedShutdownAuthority::Host),
                },
                ClosedShutdownAuthority::Token(token) => {
                    match ZeroProcessProof::from_supervisor(owner.clone(), &token) {
                        Ok(proof) => ClosedShutdownAuthority::Release(Some(proof)),
                        Err(_) => return Err(ClosedShutdownAuthority::Token(token)),
                    }
                }
                ClosedShutdownAuthority::Release(proof) => {
                    let release = if recovering {
                        self.runtime.release_recovered(owner, host)
                    } else {
                        match self.runtime.release(owner, host) {
                            Ok(()) => Ok(()),
                            Err(super::super::runtime_slot::RuntimeSlotError::Recovery) => {
                                self.runtime.release_recovered(owner, host)
                            }
                            Err(error) => Err(error),
                        }
                    };
                    return release.map_err(|_| ClosedShutdownAuthority::Release(proof));
                }
            };
        }
    }

    fn take_live_host_recovery(&self, owner: &RuntimeOwner) -> Option<LiveHostRecovery> {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let recovery = retained.take()?;
        match recovery {
            PendingCodexRecovery::LiveHost(recovery) if &recovery.owner == owner => Some(recovery),
            recovery => {
                *retained = Some(recovery);
                None
            }
        }
    }

    fn has_runtime_recovery_for(&self, owner: &RuntimeOwner) -> bool {
        let retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        matches!(
            retained.as_ref(),
            Some(PendingCodexRecovery::RuntimeZeroToken {
                owner: retained_owner,
                ..
            } | PendingCodexRecovery::RuntimeProjection {
                owner: retained_owner,
                ..
            }) if retained_owner == owner
        )
    }

    fn retain_live_host_error(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
        phase: LiveHostRecoveryPhase,
        error: crate::acp::runtime::HostError,
    ) -> CodexSessionError {
        if requires_live_recovery(&error) {
            let _ = self.runtime.mark_recovery(&owner);
            self.retain_live_recovery(LiveHostRecovery { owner, host, phase });
        }
        map_host_error(error)
    }

    fn retain_live_recovery(&self, recovery: LiveHostRecovery) {
        self.retain_codex_recovery(PendingCodexRecovery::LiveHost(recovery));
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
