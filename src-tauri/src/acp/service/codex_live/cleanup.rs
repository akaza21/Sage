use super::*;

impl AcpService {
    #[allow(clippy::result_large_err)]
    pub(super) fn shutdown_journal_after_startup_host(
        &self,
        abort: ActivationAbortIntent,
        category: ActivationAbortCategory,
        journal: CoordinatorActivationJournal,
        proof: crate::acp::process::ZeroProcessToken,
        _claimed: bool,
    ) -> Result<(), PendingCodexRecovery> {
        match journal.into_shutdown_handoff() {
            CoordinatorShutdownHandoff::Runtime(lease) => {
                drop(lease);
                self.shutdown_runtime_after_token(abort.owner, proof)
            }
            CoordinatorShutdownHandoff::Activation(lease) => {
                self.shutdown_activation_after_token(abort, category, lease, proof)
            }
        }
    }

    pub(super) fn release_startup_host(
        &self,
        owner: &RuntimeOwner,
        host: &Arc<RuntimeHost>,
        claimed: bool,
    ) -> Result<(), crate::acp::service::runtime_slot::RuntimeSlotError> {
        if claimed {
            self.runtime.abort_claimed_start(owner, host)
        } else {
            self.runtime.abort_start(owner)
        }
    }

    #[allow(clippy::result_large_err)]
    pub(super) fn release_runtime_slot(
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

    pub(super) async fn abort_failed_launch(
        &self,
        abort: &ActivationAbortIntent,
        mut session_lease: crate::acp::runtime::SessionTreeLease,
        failure: crate::acp::process::LaunchFailure,
        category: ActivationAbortCategory,
    ) -> Result<(), CodexSessionError> {
        let launch_error = failure.error();
        let token = match failure.into_zero_process() {
            Ok(token) => token,
            Err(failure) => {
                let _ = self.runtime.mark_recovery(&abort.owner);
                if let Ok((_, recovery)) = failure.into_recovery() {
                    self.retain_codex_recovery(PendingCodexRecovery::Launch {
                        abort: abort.clone(),
                        category,
                        lease: session_lease,
                        recovery,
                    });
                }
                return Err(CodexSessionError::Recovery);
            }
        };
        let proof = match crate::acp::runtime::ZeroProcessProof::from_supervisor(
            abort.owner.clone(),
            &token,
        ) {
            Ok(proof) => proof,
            Err(_) => {
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::ZeroToken {
                    abort: abort.clone(),
                    category,
                    lease: session_lease,
                    token,
                });
                return Err(CodexSessionError::Recovery);
            }
        };
        let command = abort.command(category);
        match session_lease
            .abort_activation_after_zero_process_retained(command, proof)
            .await
        {
            Ok(_) => {
                self.runtime
                    .abort_start(&abort.owner)
                    .map_err(map_slot_error)?;
                Err(CodexSessionError::LaunchFailure(launch_error))
            }
            Err((_error, proof)) => {
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::Abort {
                    abort: abort.clone(),
                    category,
                    lease: session_lease,
                    proof,
                });
                Err(CodexSessionError::Recovery)
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // Explicit ownership-bearing cleanup inputs.
    pub(super) async fn abort_pre_runner(
        &self,
        abort: &ActivationAbortIntent,
        mut session_lease: crate::acp::runtime::SessionTreeLease,
        stdin: &mut StdinEndpoint,
        control: &TreeControl,
        cleanup_deadline: Instant,
        category: ActivationAbortCategory,
        launch_setup: super::super::CodexLaunchSetupError,
        active: Option<ActiveCodexOperation>,
    ) -> Result<(), CodexSessionError> {
        stdin.close();
        let token = match control.terminate_zero_before(TreeExit::ParentShutdown, cleanup_deadline)
        {
            Ok((_, token)) => token,
            Err(_) => {
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::PreRunner {
                    abort: abort.clone(),
                    category,
                    lease: session_lease,
                    control: control.clone(),
                });
                if let Some(active) = active {
                    active.settle(Err(
                        crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
                    ));
                }
                return Err(CodexSessionError::Recovery);
            }
        };
        let proof = match crate::acp::runtime::ZeroProcessProof::from_supervisor(
            abort.owner.clone(),
            &token,
        ) {
            Ok(proof) => proof,
            Err(_) => {
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::ZeroToken {
                    abort: abort.clone(),
                    category,
                    lease: session_lease,
                    token,
                });
                if let Some(active) = active {
                    active.settle(Err(
                        crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
                    ));
                }
                return Err(CodexSessionError::Recovery);
            }
        };
        let aborted = session_lease
            .abort_activation_after_zero_process_retained(abort.command(category), proof)
            .await;
        if let Err((_error, proof)) = aborted {
            let _ = self.runtime.mark_recovery(&abort.owner);
            self.retain_codex_recovery(PendingCodexRecovery::Abort {
                abort: abort.clone(),
                category,
                lease: session_lease,
                proof,
            });
            if let Some(active) = active {
                active.settle(Err(
                    crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
                ));
            }
            return Err(CodexSessionError::Recovery);
        }
        if self.runtime.abort_start(&abort.owner).is_err() {
            let _ = self.runtime.mark_recovery(&abort.owner);
            if let Some(active) = active {
                active.settle(Err(
                    crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
                ));
            }
            return Err(CodexSessionError::Recovery);
        }
        if let Some(active) = active {
            active.settle(Ok(()));
        }
        Err(CodexSessionError::LaunchSetup(launch_setup))
    }

    #[allow(clippy::too_many_arguments)] // Explicit ownership-bearing cleanup inputs.
    pub(super) async fn abort_driver_before_runner(
        &self,
        abort: &ActivationAbortIntent,
        mut driver: CodexActivationDriver,
        cleanup_deadline: Instant,
        category: ActivationAbortCategory,
        launch_setup: super::super::CodexLaunchSetupError,
        host: Option<(Arc<RuntimeHost>, bool)>,
        active: Option<ActiveCodexOperation>,
    ) -> Result<(), CodexSessionError> {
        driver.fail_before_runner();
        let handoff = match driver.take_zero_process_handoff_before(cleanup_deadline) {
            Ok(handoff) => handoff,
            Err(failure) => {
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::Driver {
                    abort: abort.clone(),
                    category,
                    driver: failure.driver,
                });
                if let Some(active) = active {
                    active.settle(Err(
                        crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
                    ));
                }
                return Err(CodexSessionError::Recovery);
            }
        };
        self.finish_driver_abort(abort, handoff, category, launch_setup, host, active)
            .await
    }

    pub(super) async fn finish_driver_abort(
        &self,
        abort: &ActivationAbortIntent,
        handoff: ActivationZeroProcessHandoff<
            NativeRuntime,
            CoordinatorActivationJournal,
            NativeResourceMapper,
            crate::acp::process::ZeroProcessToken,
        >,
        category: ActivationAbortCategory,
        launch_setup: super::super::CodexLaunchSetupError,
        host: Option<(Arc<RuntimeHost>, bool)>,
        active: Option<ActiveCodexOperation>,
    ) -> Result<(), CodexSessionError> {
        let (_, journal, _) = handoff.parts.into_parts();
        let mut lease = match journal.into_teardown_lease() {
            Ok(lease) => lease,
            Err(journal) => {
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::Journal {
                    abort: abort.clone(),
                    category,
                    journal: *journal,
                    proof: handoff.proof,
                    host,
                });
                return Err(CodexSessionError::Recovery);
            }
        };
        let proof = match ZeroProcessProof::from_supervisor(abort.owner.clone(), &handoff.proof) {
            Ok(proof) => proof,
            Err(_) => {
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::ZeroToken {
                    abort: abort.clone(),
                    category,
                    lease,
                    token: handoff.proof,
                });
                return Err(CodexSessionError::Recovery);
            }
        };
        if let Err((_error, proof)) = lease
            .abort_activation_after_zero_process_retained(abort.command(category), proof)
            .await
        {
            let _ = self.runtime.mark_recovery(&abort.owner);
            self.retain_codex_recovery(PendingCodexRecovery::Abort {
                abort: abort.clone(),
                category,
                lease,
                proof,
            });
            return Err(CodexSessionError::Recovery);
        }
        if let Some((host, claimed)) = host {
            if host.abandon_startup().is_err() {
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::StartupHost {
                    owner: abort.owner.clone(),
                    host,
                    claimed,
                });
                return Err(CodexSessionError::Recovery);
            }
            if claimed {
                self.runtime
                    .abort_claimed_start(&abort.owner, &host)
                    .map_err(map_slot_error)?;
            } else {
                self.runtime
                    .abort_start(&abort.owner)
                    .map_err(map_slot_error)?;
            }
        } else {
            self.runtime
                .abort_start(&abort.owner)
                .map_err(map_slot_error)?;
        }
        if let Some(active) = active {
            active.settle(Ok(()));
        }
        Err(CodexSessionError::LaunchSetup(launch_setup))
    }

    pub(super) async fn recover_runner_start(
        &self,
        abort: &ActivationAbortIntent,
        failure: ActivationStartFailure<NativeRuntime, NativeResourceMapper, StdinEndpoint>,
        host: Arc<RuntimeHost>,
        runner: Arc<LiveRunnerHandle>,
        active: Option<ActiveCodexOperation>,
    ) -> Result<(), CodexSessionError> {
        match failure {
            ActivationStartFailure::Teardown {
                handoff, substep, ..
            } => {
                tracing::warn!(runner_start_substep = ?substep, "Codex live runner activation failed");
                self.finish_driver_abort(
                    abort,
                    handoff,
                    ActivationAbortCategory::RuntimeHandoffFailure,
                    substep.into(),
                    Some((host, true)),
                    active,
                )
                .await
            }
            ActivationStartFailure::Projection {
                handoff, substep, ..
            } => {
                tracing::warn!(runner_start_substep = ?substep, "Codex live runner activation failed");
                let (_, journal, _) = handoff.parts.into_parts();
                self.recover_projection_authorities(abort, journal, handoff.proof, host, active)
            }
            ActivationStartFailure::Retained {
                driver, substep, ..
            } => {
                tracing::warn!(runner_start_substep = ?substep, "Codex live runner activation failed");
                if let Some(active) = active {
                    active.settle(Err(
                        crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
                    ));
                }
                let _ = self.runtime.mark_recovery(&abort.owner);
                let _ = host.abandon_startup();
                self.retain_codex_recovery(PendingCodexRecovery::Driver {
                    abort: abort.clone(),
                    category: ActivationAbortCategory::RuntimeHandoffFailure,
                    driver,
                });
                Err(CodexSessionError::Recovery)
            }
            ActivationStartFailure::OwnedByRunner { substep, .. } => {
                tracing::warn!(runner_start_substep = ?substep, "Codex live runner activation failed");
                if let Some(active) = active {
                    active.settle(Err(
                        crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup,
                    ));
                }
                let _ = self.runtime.mark_recovery(&abort.owner);
                self.retain_codex_recovery(PendingCodexRecovery::Runner {
                    owner: abort.owner.clone(),
                    runner,
                });
                Err(CodexSessionError::Recovery)
            }
        }
    }

    pub(super) fn release_activation_slot(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<(), crate::acp::service::runtime_slot::RuntimeSlotError> {
        self.runtime
            .abort_start(owner)
            .or_else(|_| self.runtime.recover_after_zero_process(owner))
    }
}
