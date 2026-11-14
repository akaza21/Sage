use super::live_recovery::{ClosedShutdownAuthority, LiveHostRecovery, LiveHostRecoveryPhase};
use super::*;

impl Lane<'_> {
    pub(crate) fn cancel_session_turn(
        &self,
        intent: crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let owner = intent.owner.clone();
        let turn_id = intent.turn_id.clone();
        let host = match self.runtime.for_owner(&owner) {
            Ok(host) => host,
            Err(super::super::runtime_slot::RuntimeSlotError::Recovery) => {
                let deadline = Instant::now()
                    + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close);
                let _ = self.shutdown_codex_recovery(deadline);
                return self.settled_cancel_outcome(&owner, &turn_id);
            }
            Err(error) => return Err(map_slot_error(error)),
        };
        match host.cancel(intent) {
            // Dispatching cancellation durably marks this exact turn as
            // Cancelling. The native terminal arrives asynchronously; waiting
            // for it here incorrectly turns an admitted owner's cancellation
            // into recovery solely because the runner has not replied yet.
            Ok(outcome) => self.finish_cancelled_shutdown(owner, turn_id, host, outcome),
            Err(crate::acp::runtime::HostError::Intent | crate::acp::runtime::HostError::State)
                if self.host_failed_closed(&host) =>
            {
                self.settle_cancel_on_failed_host(owner, turn_id, host)
            }
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
            + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close);
        #[cfg(all(test, windows))]
        self.record_cancel_stage("durable-wait");
        while !self.cancel_is_durable(&owner, &turn_id) {
            if Instant::now() >= deadline {
                #[cfg(all(test, windows))]
                self.record_cancel_stage("durable-timeout");
                return Ok(outcome);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        #[cfg(all(test, windows))]
        self.record_cancel_stage("converge");
        match self.converge_closed_shutdown(
            &owner,
            &host,
            ClosedShutdownAuthority::Host,
            false,
            deadline,
        ) {
            Ok(()) => {
                #[cfg(all(test, windows))]
                self.record_cancel_stage("park");
                self.park_after_cancel_zero(&owner).map(|()| {
                    #[cfg(all(test, windows))]
                    self.record_cancel_stage("ok");
                    outcome
                })
            }
            Err(_) => {
                #[cfg(all(test, windows))]
                self.record_cancel_stage("converge");
                self.retain_live_failure_recovery(owner, host, LiveHostRecoveryPhase::CancelEffect);
                Err(CodexSessionError::Recovery)
            }
        }
    }

    pub(crate) fn close_session(
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
                    + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close);
                let _ = self.shutdown_codex_recovery(deadline);
            }
            return Err(CodexSessionError::Recovery);
        };
        if recovery.owner != owner {
            let host = Arc::clone(&recovery.host);
            self.replace_live_host_inflight_recovery(
                &owner,
                &host,
                Some(PendingCodexRecovery::LiveHost(recovery)),
            );
            return Err(CodexSessionError::Intent);
        }
        let LiveHostRecovery { host, phase, .. } = recovery;
        match phase {
            LiveHostRecoveryPhase::ReplacementPark
            | LiveHostRecoveryPhase::NativeEnd
            | LiveHostRecoveryPhase::NativeEndInFlight => Err(CodexSessionError::Recovery),
            phase @ (LiveHostRecoveryPhase::SendEffect
            | LiveHostRecoveryPhase::CancelEffect
            | LiveHostRecoveryPhase::ConfigEffect) => {
                let deadline = Instant::now()
                    + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close);
                if let Some(recovery) = self.shutdown_live_host_recovery(
                    LiveHostRecovery {
                        owner: owner.clone(),
                        host: Arc::clone(&host),
                        phase,
                    },
                    deadline,
                ) {
                    self.replace_live_host_inflight_recovery(&owner, &host, Some(recovery));
                } else {
                    self.replace_live_host_inflight_recovery(&owner, &host, None);
                }
                Err(CodexSessionError::Recovery)
            }
            LiveHostRecoveryPhase::CloseEffect {
                intent: retained_intent,
            } => {
                if retained_intent != intent {
                    self.replace_live_host_inflight_recovery(
                        &owner,
                        &host,
                        Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                            owner: owner.clone(),
                            host: Arc::clone(&host),
                            phase: LiveHostRecoveryPhase::CloseEffect {
                                intent: retained_intent,
                            },
                        })),
                    );
                    return Err(CodexSessionError::Intent);
                }
                match host.recover_close(intent.clone()) {
                    Ok(outcome) => self.finish_recovered_closed_authority(
                        owner.clone(),
                        Arc::clone(&host),
                        intent,
                        outcome,
                        ClosedShutdownAuthority::Host,
                    ),
                    Err(error) => {
                        self.replace_live_host_inflight_recovery(
                            &owner,
                            &host,
                            Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                                owner: owner.clone(),
                                host: Arc::clone(&host),
                                phase: LiveHostRecoveryPhase::CloseEffect { intent },
                            })),
                        );
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
                    self.replace_live_host_inflight_recovery(
                        &owner,
                        &host,
                        Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                            owner: owner.clone(),
                            host: Arc::clone(&host),
                            phase: LiveHostRecoveryPhase::Closed {
                                intent: retained_intent,
                                outcome,
                                authority,
                            },
                        })),
                    );
                    return Err(CodexSessionError::Intent);
                }
                self.finish_recovered_closed_authority(
                    owner.clone(),
                    Arc::clone(&host),
                    intent,
                    outcome,
                    authority,
                )
            }
        }
    }

    fn finish_recovered_closed_authority(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
        intent: crate::acp::runtime::HostCloseIntent,
        outcome: crate::acp::runtime::HostOutcome,
        authority: ClosedShutdownAuthority,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let deadline = Instant::now()
            + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close);
        match self.converge_closed_shutdown(&owner, &host, authority, true, deadline) {
            Ok(()) => {
                let result = crate::app_preview_commands::retire_preview_runtime_owner(&owner)
                    .map_err(|_| CodexSessionError::Recovery)
                    .map(|()| outcome);
                self.replace_live_host_inflight_recovery(&owner, &host, None);
                result
            }
            Err(authority) => {
                self.replace_live_host_inflight_recovery(
                    &owner,
                    &host,
                    Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                        owner: owner.clone(),
                        host: Arc::clone(&host),
                        phase: LiveHostRecoveryPhase::Closed {
                            intent,
                            outcome,
                            authority,
                        },
                    })),
                );
                Err(CodexSessionError::Recovery)
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
            + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close);
        match self.converge_closed_shutdown(&owner, &host, authority, recovering, deadline) {
            Ok(()) => {
                crate::app_preview_commands::retire_preview_runtime_owner(&owner)
                    .map_err(|_| CodexSessionError::Recovery)?;
                Ok(outcome)
            }
            Err(authority) => {
                self.retain_live_failure_recovery(
                    owner,
                    host,
                    LiveHostRecoveryPhase::Closed {
                        intent,
                        outcome,
                        authority,
                    },
                );
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
}
