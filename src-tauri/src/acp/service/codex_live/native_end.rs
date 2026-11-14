use super::live_recovery::{LiveHostRecovery, LiveHostRecoveryPhase};
use super::*;

impl Lane<'_> {
    pub(super) fn native_end_recovery_owner(
        &self,
    ) -> Result<Option<RuntimeOwner>, crate::acp::runtime::HostError> {
        let retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match retained.as_ref() {
            Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                owner,
                phase: LiveHostRecoveryPhase::NativeEnd,
                ..
            })) => Ok(Some(owner.clone())),
            Some(
                PendingCodexRecovery::LiveHost(LiveHostRecovery {
                    phase: LiveHostRecoveryPhase::NativeEndInFlight,
                    ..
                })
                | PendingCodexRecovery::LiveHostInFlight { .. },
            ) => Err(crate::acp::runtime::HostError::RecoveryRequired),
            None | Some(_) => Ok(None),
        }
    }

    pub(crate) fn end_session_natively(
        &self,
        owner: RuntimeOwner,
    ) -> Result<(), CodexSessionError> {
        let host = self.arm_native_end(&owner)?;
        let deadline = Instant::now()
            + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close);
        match host.shutdown_recovery_before(deadline) {
            Ok(Some(token)) => match self.shutdown_runtime_after_token(owner.clone(), token) {
                Ok(()) => self.finish_native_end(&owner, &host),
                Err(recovery) => self.replace_native_end_recovery(&owner, &host, recovery),
            },
            Ok(None) | Err(_) => self.replace_native_end_recovery(
                &owner,
                &host,
                PendingCodexRecovery::LiveHost(LiveHostRecovery {
                    owner: owner.clone(),
                    host: Arc::clone(&host),
                    phase: LiveHostRecoveryPhase::NativeEnd,
                }),
            ),
        }
    }

    fn arm_native_end(&self, owner: &RuntimeOwner) -> Result<Arc<RuntimeHost>, CodexSessionError> {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match retained.as_ref() {
            Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                owner: current,
                host,
                phase: LiveHostRecoveryPhase::NativeEnd,
            })) if current == owner => {
                let host = Arc::clone(host);
                *retained = Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                    owner: owner.clone(),
                    host: Arc::clone(&host),
                    phase: LiveHostRecoveryPhase::NativeEndInFlight,
                }));
                Ok(host)
            }
            Some(_) => Err(CodexSessionError::Recovery),
            None => {
                let host = self.runtime.for_owner(owner).map_err(map_slot_error)?;
                if !host.has_shutdown_recovery() {
                    return Err(CodexSessionError::Recovery);
                }
                self.runtime.mark_recovery(owner).map_err(map_slot_error)?;
                *retained = Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                    owner: owner.clone(),
                    host: Arc::clone(&host),
                    phase: LiveHostRecoveryPhase::NativeEndInFlight,
                }));
                Ok(host)
            }
        }
    }

    fn replace_native_end_recovery(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
        recovery: PendingCodexRecovery,
    ) -> Result<(), CodexSessionError> {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let replace = match retained.as_ref() {
            Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                owner: current,
                host: current_host,
                phase: LiveHostRecoveryPhase::NativeEndInFlight,
                ..
            })) => current == owner && Arc::ptr_eq(current_host, host),
            None | Some(_) => false,
        };
        if !replace {
            return Err(CodexSessionError::Recovery);
        }
        *retained = Some(recovery);
        Err(CodexSessionError::Recovery)
    }

    fn finish_native_end(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
    ) -> Result<(), CodexSessionError> {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match retained.as_ref() {
            Some(PendingCodexRecovery::LiveHost(LiveHostRecovery {
                owner: current,
                host: current_host,
                phase: LiveHostRecoveryPhase::NativeEndInFlight,
            })) if current == owner && Arc::ptr_eq(current_host, host) => {
                retained.take();
                Ok(())
            }
            None | Some(_) => Err(CodexSessionError::Recovery),
        }
    }
}
