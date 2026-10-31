use super::*;

mod pending;
pub(in crate::acp::service) use pending::PendingCodexRecovery;

impl Lane<'_> {
    pub(super) fn retain_codex_recovery(&self, recovery: PendingCodexRecovery) {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if retained.is_none() {
            *retained = Some(recovery);
        }
    }

    /// Releases a stale runner recovery only after a passive native zero-tree
    /// check. A still-live tree remains retained for explicit recovery, so a
    /// new admission cannot bypass containment.
    pub(super) fn reclaim_zero_process_runner_capacity(
        &self,
        deadline: Instant,
    ) -> Result<bool, crate::acp::runtime::HostError> {
        let recovery = {
            let mut retained = self
                .codex_recovery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !matches!(retained.as_ref(), Some(PendingCodexRecovery::Runner { .. })) {
                return Ok(false);
            }
            retained.take()
        };
        let Some(recovery) = recovery else {
            return Ok(false);
        };
        let PendingCodexRecovery::Runner { owner, runner } = recovery else {
            self.retain_codex_recovery(recovery);
            return Ok(false);
        };
        match runner.reclaim_if_zero_before(deadline) {
            Ok(token) => match self.shutdown_runtime_after_token(owner.clone(), token) {
                Ok(()) => Ok(true),
                Err(recovery) => {
                    self.retain_codex_recovery(recovery);
                    Err(crate::acp::runtime::HostError::RecoveryRequired)
                }
            },
            Err(error) => {
                self.retain_codex_recovery(PendingCodexRecovery::Runner { owner, runner });
                Err(error)
            }
        }
    }

    pub(in crate::acp::service) fn reclaim_zero_process_capacity(
        &self,
        deadline: Instant,
    ) -> Result<bool, crate::acp::runtime::HostError> {
        if self.reclaim_zero_process_runner_capacity(deadline)? {
            return Ok(true);
        }
        self.shutdown_codex_recovery(deadline)
    }

    pub(in crate::acp::service) fn shutdown_codex_recovery(
        &self,
        deadline: Instant,
    ) -> Result<bool, crate::acp::runtime::HostError> {
        if let Some(owner) = self.native_end_recovery_owner()? {
            return self
                .end_session_natively(owner)
                .map(|()| true)
                .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired);
        }
        if let Some(reclaimed) = self.shutdown_retained_live_host_recovery(deadline)? {
            return Ok(reclaimed);
        }
        let recovery = {
            let mut retained = self
                .codex_recovery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if matches!(
                retained.as_ref(),
                Some(
                    PendingCodexRecovery::LiveHost(_)
                        | PendingCodexRecovery::LiveHostInFlight { .. }
                )
            ) {
                return Err(crate::acp::runtime::HostError::RecoveryRequired);
            }
            retained.take()
        };
        let Some(recovery) = recovery else {
            return Ok(false);
        };
        let retained = match recovery {
            PendingCodexRecovery::Activation { recovery } => recovery
                .shutdown_before_process_launch()
                .err()
                .map(|recovery| PendingCodexRecovery::Activation { recovery }),
            PendingCodexRecovery::Launch {
                abort,
                category,
                lease,
                recovery,
            } => match recovery.retry_zero() {
                Ok(token) => self
                    .shutdown_activation_after_token(abort, category, lease, token)
                    .err(),
                Err(recovery) => Some(PendingCodexRecovery::Launch {
                    abort,
                    category,
                    lease,
                    recovery: *recovery,
                }),
            },
            PendingCodexRecovery::PreRunner {
                abort,
                category,
                lease,
                control,
            } => match control.terminate_zero_before(TreeExit::ParentShutdown, deadline) {
                Ok((_, token)) => self
                    .shutdown_activation_after_token(abort, category, lease, token)
                    .err(),
                Err(_) => Some(PendingCodexRecovery::PreRunner {
                    abort,
                    category,
                    lease,
                    control,
                }),
            },
            PendingCodexRecovery::ZeroToken {
                abort,
                category,
                lease,
                token,
            } => self
                .shutdown_activation_after_token(abort, category, lease, token)
                .err(),
            PendingCodexRecovery::Abort {
                abort,
                category,
                mut lease,
                proof,
            } => match lease.abort_activation_after_zero_process_blocking_retained(
                abort.command(category),
                proof,
            ) {
                Ok(_) => self
                    .release_activation_slot(&abort.owner)
                    .err()
                    .map(|_| PendingCodexRecovery::ActivationCommitted { owner: abort.owner }),
                Err((_, proof)) => Some(PendingCodexRecovery::Abort {
                    abort,
                    category,
                    lease,
                    proof,
                }),
            },
            PendingCodexRecovery::ActivationCommitted { owner } => self
                .release_activation_slot(&owner)
                .err()
                .map(|_| PendingCodexRecovery::ActivationCommitted { owner }),
            PendingCodexRecovery::Journal {
                abort,
                category,
                journal,
                proof,
                host,
            } => {
                if let Some((host, claimed)) = host {
                    if host.abandon_startup().is_err() {
                        Some(PendingCodexRecovery::Journal {
                            abort,
                            category,
                            journal,
                            proof,
                            host: Some((host, claimed)),
                        })
                    } else {
                        self.shutdown_journal_after_startup_host(
                            abort, category, journal, proof, claimed,
                        )
                        .err()
                    }
                } else {
                    self.shutdown_journal_after_startup_host(abort, category, journal, proof, false)
                        .err()
                }
            }
            PendingCodexRecovery::StartupHost {
                owner,
                host,
                claimed,
            } => match host.abandon_startup() {
                Ok(()) => self
                    .release_startup_host(&owner, &host, claimed)
                    .err()
                    .map(|_| PendingCodexRecovery::StartupHost {
                        owner,
                        host,
                        claimed,
                    }),
                Err(_) => Some(PendingCodexRecovery::StartupHost {
                    owner,
                    host,
                    claimed,
                }),
            },
            PendingCodexRecovery::LiveHost(_) | PendingCodexRecovery::LiveHostInFlight { .. } => {
                unreachable!()
            }
            PendingCodexRecovery::Driver {
                abort,
                category,
                mut driver,
            } => {
                driver.fail_before_runner();
                match driver.take_zero_process_handoff_before(deadline) {
                    Ok(handoff) => {
                        let category = handoff.cause.activation_abort_category(category);
                        let (pump, journal, _) = handoff.parts.into_parts();
                        drop(pump);
                        match journal.into_shutdown_handoff() {
                            CoordinatorShutdownHandoff::Runtime(lease) => {
                                drop(lease);
                                self.shutdown_runtime_after_token(abort.owner, handoff.proof)
                                    .err()
                            }
                            CoordinatorShutdownHandoff::Activation(lease) => self
                                .shutdown_activation_after_token(
                                    abort,
                                    category,
                                    lease,
                                    handoff.proof,
                                )
                                .err(),
                        }
                    }
                    Err(failure) => Some(PendingCodexRecovery::Driver {
                        abort,
                        category,
                        driver: failure.driver,
                    }),
                }
            }
            PendingCodexRecovery::Projection {
                owner,
                lease,
                proof,
                host,
            } => match host.abandon_startup() {
                Ok(()) => match self
                    .sessions
                    .recover_runtime_after_zero_process_retained(owner.clone(), proof)
                {
                    Ok(proof) => {
                        drop(lease);
                        self.release_runtime_slot(owner, proof).err()
                    }
                    Err((_, proof)) => Some(PendingCodexRecovery::Projection {
                        owner,
                        lease,
                        proof,
                        host,
                    }),
                },
                Err(_) => Some(PendingCodexRecovery::Projection {
                    owner,
                    lease,
                    proof,
                    host,
                }),
            },
            PendingCodexRecovery::ProjectionToken {
                owner,
                lease,
                token,
                host,
            } => match host.abandon_startup() {
                Ok(()) => match ZeroProcessProof::from_supervisor(owner.clone(), &token) {
                    Ok(proof) => Some(PendingCodexRecovery::Projection {
                        owner,
                        lease,
                        proof,
                        host,
                    }),
                    Err(_) => Some(PendingCodexRecovery::ProjectionToken {
                        owner,
                        lease,
                        token,
                        host,
                    }),
                },
                Err(_) => Some(PendingCodexRecovery::ProjectionToken {
                    owner,
                    lease,
                    token,
                    host,
                }),
            },
            PendingCodexRecovery::Runner { owner, runner } => {
                match runner.shutdown_recovery_before(deadline) {
                    Ok(token) => self.shutdown_runtime_after_token(owner, token).err(),
                    Err(_) => Some(PendingCodexRecovery::Runner { owner, runner }),
                }
            }
            PendingCodexRecovery::RuntimeZeroToken { owner, token } => {
                self.shutdown_runtime_after_token(owner, token).err()
            }
            PendingCodexRecovery::RuntimeProjection { owner, proof } => self
                .sessions
                .recover_runtime_after_zero_process_retained(owner.clone(), proof)
                .map_err(|(_, proof)| proof)
                .and_then(|proof| {
                    self.release_runtime_slot(owner.clone(), proof)
                        .map_err(|recovery| match recovery {
                            PendingCodexRecovery::RuntimeProjection { proof, .. } => proof,
                            _ => unreachable!(),
                        })
                })
                .err()
                .map(|proof| PendingCodexRecovery::RuntimeProjection { owner, proof }),
        };
        if let Some(recovery) = retained {
            self.retain_codex_recovery(recovery);
            return Err(crate::acp::runtime::HostError::RecoveryRequired);
        }
        Ok(true)
    }

    #[allow(clippy::result_large_err)]
    pub(super) fn shutdown_runtime_after_token(
        &self,
        owner: RuntimeOwner,
        token: crate::acp::process::ZeroProcessToken,
    ) -> Result<(), PendingCodexRecovery> {
        let proof = ZeroProcessProof::from_supervisor(owner.clone(), &token).map_err(|_| {
            PendingCodexRecovery::RuntimeZeroToken {
                owner: owner.clone(),
                token,
            }
        })?;
        let proof = self
            .sessions
            .recover_runtime_after_zero_process_retained(owner.clone(), proof)
            .map_err(|(_, proof)| PendingCodexRecovery::RuntimeProjection {
                owner: owner.clone(),
                proof,
            })?;
        self.release_runtime_slot(owner, proof)
    }

    #[allow(clippy::result_large_err)]
    pub(super) fn shutdown_activation_after_token(
        &self,
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        token: crate::acp::process::ZeroProcessToken,
    ) -> Result<(), PendingCodexRecovery> {
        let proof = match ZeroProcessProof::from_supervisor(abort.owner.clone(), &token) {
            Ok(proof) => proof,
            Err(_) => {
                return Err(PendingCodexRecovery::ZeroToken {
                    abort,
                    category,
                    lease,
                    token,
                });
            }
        };
        let mut lease = lease;
        match lease
            .abort_activation_after_zero_process_blocking_retained(abort.command(category), proof)
        {
            Ok(_) => {
                drop(lease);
                self.release_activation_slot(&abort.owner)
                    .map_err(|_| PendingCodexRecovery::ActivationCommitted { owner: abort.owner })
            }
            Err((_, proof)) => Err(PendingCodexRecovery::Abort {
                abort,
                category,
                lease,
                proof,
            }),
        }
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
