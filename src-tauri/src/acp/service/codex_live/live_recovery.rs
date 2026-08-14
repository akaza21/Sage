use super::*;

pub(in crate::acp::service) struct LiveHostRecovery {
    pub(in crate::acp::service) owner: RuntimeOwner,
    pub(in crate::acp::service) host: Arc<RuntimeHost>,
    pub(in crate::acp::service) phase: LiveHostRecoveryPhase,
}

pub(in crate::acp::service) enum LiveHostRecoveryPhase {
    SendEffect,
    CancelEffect,
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
