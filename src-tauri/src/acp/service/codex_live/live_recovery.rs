use super::*;

pub(in crate::acp::service) struct LiveHostRecovery {
    pub(in crate::acp::service) owner: RuntimeOwner,
    pub(in crate::acp::service) host: Arc<RuntimeHost>,
    pub(in crate::acp::service) phase: LiveHostRecoveryPhase,
}

pub(in crate::acp::service) enum LiveHostRecoveryPhase {
    SendEffect,
    CancelEffect,
    ConfigEffect,
    ReplacementPark,
    NativeEnd,
    NativeEndInFlight,
    CloseEffect {
        intent: crate::acp::runtime::HostCloseIntent,
    },
    Closed {
        intent: crate::acp::runtime::HostCloseIntent,
        outcome: crate::acp::runtime::HostOutcome,
        authority: ClosedShutdownAuthority,
    },
}

pub(in crate::acp::service) enum ClosedShutdownAuthority {
    Host,
    Token(crate::acp::process::ZeroProcessToken),
    Release(Option<ZeroProcessProof>),
}

impl AcpService {
    pub(super) fn take_live_host_recovery(&self, owner: &RuntimeOwner) -> Option<LiveHostRecovery> {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(PendingCodexRecovery::LiveHost(recovery)) = retained.as_ref() else {
            return None;
        };
        if &recovery.owner != owner
            || matches!(
                recovery.phase,
                LiveHostRecoveryPhase::ReplacementPark
                    | LiveHostRecoveryPhase::NativeEnd
                    | LiveHostRecoveryPhase::NativeEndInFlight
            )
        {
            return None;
        }
        let recovery = match retained.take() {
            Some(PendingCodexRecovery::LiveHost(recovery)) => recovery,
            _ => unreachable!(),
        };
        *retained = Some(PendingCodexRecovery::LiveHostInFlight {
            owner: recovery.owner.clone(),
            host: Arc::clone(&recovery.host),
        });
        Some(recovery)
    }

    pub(super) fn replace_live_host_inflight_recovery(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
        replacement: Option<PendingCodexRecovery>,
    ) -> bool {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !matches!(
            retained.as_ref(),
            Some(PendingCodexRecovery::LiveHostInFlight {
                owner: current_owner,
                host: current_host,
            }) if current_owner == owner && Arc::ptr_eq(current_host, host)
        ) {
            return false;
        }
        *retained = replacement;
        true
    }

    pub(super) fn has_runtime_recovery_for(&self, owner: &RuntimeOwner) -> bool {
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

    #[cfg(all(test, windows))]
    pub(crate) fn has_retained_live_host_recovery_for(&self, owner: &RuntimeOwner) -> bool {
        let retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        matches!(
            retained.as_ref(),
            Some(PendingCodexRecovery::LiveHost(recovery)) if &recovery.owner == owner
        )
    }

    pub(super) fn retain_live_host_error(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
        phase: LiveHostRecoveryPhase,
        error: crate::acp::runtime::HostError,
    ) -> CodexSessionError {
        tracing::warn!(
            recovery_checkpoint = "codex_live.send_or_cancel.host_operation",
            host_error = ?error,
            "Codex live host operation failed"
        );
        if requires_live_recovery(&error) {
            self.retain_live_failure_recovery(owner, host, phase);
        }
        map_host_error(error)
    }

    pub(in crate::acp::service) fn retain_config_effect_recovery(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    ) {
        self.retain_live_failure_recovery(owner, host, LiveHostRecoveryPhase::ConfigEffect);
    }

    pub(super) fn retain_live_failure_recovery(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
        phase: LiveHostRecoveryPhase,
    ) {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let recovery = PendingCodexRecovery::LiveHost(LiveHostRecovery {
            owner: owner.clone(),
            host: Arc::clone(&host),
            phase,
        });
        if retained.is_none() && self.runtime.mark_recovery(&owner).is_ok() {
            *retained = Some(recovery);
        }
    }

    pub(super) fn shutdown_retained_live_host_recovery(
        &self,
        deadline: Instant,
    ) -> Result<Option<bool>, crate::acp::runtime::HostError> {
        let recovery = {
            let mut retained = self
                .codex_recovery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(recovery) = retained.take() else {
                return Ok(None);
            };
            let PendingCodexRecovery::LiveHost(recovery) = recovery else {
                *retained = Some(recovery);
                return Ok(None);
            };
            if matches!(
                recovery.phase,
                LiveHostRecoveryPhase::NativeEnd | LiveHostRecoveryPhase::NativeEndInFlight
            ) {
                *retained = Some(PendingCodexRecovery::LiveHost(recovery));
                return Err(crate::acp::runtime::HostError::RecoveryRequired);
            }
            *retained = Some(PendingCodexRecovery::LiveHostInFlight {
                owner: recovery.owner.clone(),
                host: Arc::clone(&recovery.host),
            });
            recovery
        };
        let owner = recovery.owner.clone();
        let host = Arc::clone(&recovery.host);
        let replacement = self.shutdown_live_host_recovery(recovery, deadline);
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !matches!(
            retained.as_ref(),
            Some(PendingCodexRecovery::LiveHostInFlight {
                owner: current_owner,
                host: current_host,
            }) if current_owner == &owner && Arc::ptr_eq(current_host, &host)
        ) {
            return Err(crate::acp::runtime::HostError::RecoveryRequired);
        }
        *retained = replacement;
        if retained.is_some() {
            return Err(crate::acp::runtime::HostError::RecoveryRequired);
        }
        Ok(Some(true))
    }

    pub(super) fn shutdown_live_host_recovery(
        &self,
        recovery: LiveHostRecovery,
        deadline: Instant,
    ) -> Option<PendingCodexRecovery> {
        let LiveHostRecovery { owner, host, phase } = recovery;
        match phase {
            LiveHostRecoveryPhase::Closed {
                intent,
                outcome,
                authority,
            } => self
                .converge_closed_shutdown(&owner, &host, authority, true, deadline)
                .err()
                .map(|authority| {
                    PendingCodexRecovery::LiveHost(LiveHostRecovery {
                        owner,
                        host,
                        phase: LiveHostRecoveryPhase::Closed {
                            intent,
                            outcome,
                            authority,
                        },
                    })
                }),
            LiveHostRecoveryPhase::NativeEnd => match host.shutdown_recovery_before(deadline) {
                Ok(Some(token)) => self.shutdown_runtime_after_token(owner, token).err(),
                Ok(None) | Err(_) => Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                    owner,
                    host,
                    phase: LiveHostRecoveryPhase::NativeEnd,
                })),
            },
            LiveHostRecoveryPhase::NativeEndInFlight => {
                Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                    owner,
                    host,
                    phase: LiveHostRecoveryPhase::NativeEndInFlight,
                }))
            }
            LiveHostRecoveryPhase::ReplacementPark => {
                if host.shutdown(deadline).is_ok()
                    && self.scheduler.active().is_none()
                    && self.runtime.release_recovered(&owner, &host).is_ok()
                {
                    None
                } else {
                    match host.shutdown_recovery_before(deadline) {
                        Ok(Some(token)) => self.shutdown_runtime_after_token(owner, token).err(),
                        Ok(None) | Err(_) => {
                            Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                                owner,
                                host,
                                phase: LiveHostRecoveryPhase::ReplacementPark,
                            }))
                        }
                    }
                }
            }
            phase => match host.shutdown_recovery_before(deadline) {
                Ok(Some(token)) => self.shutdown_runtime_after_token(owner, token).err(),
                Ok(None) => self
                    .runtime
                    .recover_after_zero_process(&owner)
                    .err()
                    .map(|_| {
                        PendingCodexRecovery::LiveHost(LiveHostRecovery { owner, host, phase })
                    }),
                Err(_) => Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                    owner,
                    host,
                    phase,
                })),
            },
        }
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
