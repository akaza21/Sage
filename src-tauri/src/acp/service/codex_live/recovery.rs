use super::*;

// Each variant is a unique, non-cloneable recovery authority. Keeping its
// payload inline makes replacement under the single mutex atomic.
#[allow(clippy::large_enum_variant)]
pub(in crate::acp::service) enum PendingCodexRecovery {
    Activation {
        recovery: crate::acp::runtime::ActivationRecovery,
    },
    Launch {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        recovery: crate::acp::process::LaunchRecovery,
    },
    PreRunner {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        control: TreeControl,
    },
    ZeroToken {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        token: crate::acp::process::ZeroProcessToken,
    },
    Abort {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        lease: SessionTreeLease,
        proof: ZeroProcessProof,
    },
    ActivationCommitted {
        owner: RuntimeOwner,
    },
    Driver {
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        driver: CodexActivationDriver,
    },
    Projection {
        owner: RuntimeOwner,
        lease: SessionTreeLease,
        proof: ZeroProcessProof,
    },
    Runner {
        owner: RuntimeOwner,
        runner: Arc<LiveRunnerHandle>,
    },
    RuntimeZeroToken {
        owner: RuntimeOwner,
        token: crate::acp::process::ZeroProcessToken,
    },
    RuntimeProjection {
        owner: RuntimeOwner,
        proof: ZeroProcessProof,
    },
}

impl AcpService {
    pub(super) fn retain_codex_recovery(&self, recovery: PendingCodexRecovery) {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if retained.is_none() {
            *retained = Some(recovery);
        }
    }

    pub(in crate::acp::service) fn shutdown_codex_recovery(
        &self,
        deadline: Instant,
    ) -> Result<bool, crate::acp::runtime::HostError> {
        let recovery = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
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
            PendingCodexRecovery::Driver {
                abort,
                category,
                mut driver,
            } => {
                driver.fail_before_runner();
                match driver.take_zero_process_handoff_before(deadline) {
                    Ok(handoff) => {
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
            } => match self
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
    fn shutdown_runtime_after_token(
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
    fn shutdown_activation_after_token(
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

    fn release_activation_slot(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<(), crate::acp::service::runtime_slot::RuntimeSlotError> {
        self.runtime
            .abort_start(owner)
            .or_else(|_| self.runtime.recover_after_zero_process(owner))
    }

    #[allow(clippy::result_large_err)]
    fn release_runtime_slot(
        &self,
        owner: RuntimeOwner,
        proof: ZeroProcessProof,
    ) -> Result<(), PendingCodexRecovery> {
        #[cfg(test)]
        let release = if self
            .codex_runtime_release_fail_always
            .load(std::sync::atomic::Ordering::Acquire)
        {
            Err(super::super::runtime_slot::RuntimeSlotError::Recovery)
        } else {
            self.runtime.recover_after_zero_process_fail_once_for_test(
                &owner,
                &self.codex_runtime_release_fail_once,
            )
        };
        #[cfg(not(test))]
        let release = self.runtime.recover_after_zero_process(&owner);
        release.map_err(|_| PendingCodexRecovery::RuntimeProjection { owner, proof })
    }

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
        self.runtime
            .for_owner(&owner)
            .map_err(map_slot_error)?
            .send(crate::acp::runtime::HostPromptIntent { command })
            .map_err(map_host_error)
    }

    pub(crate) fn cancel_codex_turn(
        &self,
        intent: crate::acp::runtime::HostCancelIntent,
    ) -> Result<crate::acp::runtime::HostOutcome, CodexSessionError> {
        let owner = intent.owner.clone();
        self.runtime
            .for_owner(&owner)
            .map_err(map_slot_error)?
            .cancel(intent)
            .map_err(map_host_error)
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
        let host = self.runtime.for_owner(&owner).map_err(map_slot_error)?;
        let outcome = host
            .close(crate::acp::runtime::HostCloseIntent { command })
            .map_err(map_host_error)?;
        host.shutdown().map_err(map_host_error)?;
        self.runtime
            .release(&owner, &host)
            .map_err(map_slot_error)?;
        Ok(outcome)
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
