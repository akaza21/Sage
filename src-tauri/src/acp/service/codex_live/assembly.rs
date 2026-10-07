use super::*;

impl AcpService {
    #[cfg(test)]
    pub(crate) async fn activate_codex_session(
        &self,
        command: ActivateSessionCommand,
        project_root: crate::acp::bindings::codex::CodexProjectRootAuthority,
    ) -> Result<(), CodexSessionError> {
        let admitted = self
            .refresh_codex_binding()
            .map_err(CodexSessionError::Binding)?;
        self.activate_session(
            admitted,
            command,
            crate::acp::bindings::registry::BindingProjectRoot::Codex(project_root),
        )
        .await
    }

    pub(crate) async fn activate_session(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        command: ActivateSessionCommand,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
    ) -> Result<(), CodexSessionError> {
        let root = project_root.configured_resource_root();
        let (command, mut prepared) =
            self.prepare_session_activation(admitted, command, project_root)?;
        let abort = ActivationAbortIntent::from_prepared(&prepared);
        let resources = NativeResourceMapper::new([root]).map_err(|_| CodexSessionError::Intent)?;
        let (operation_deadline, cleanup_deadline) = deadlines()?;
        self.reclaim_zero_process_capacity(cleanup_deadline)
            .map_err(|_| CodexSessionError::Recovery)?;
        self.park_idle_runtime_for_replacement(cleanup_deadline)?;
        self.runtime
            .reserve_start(prepared.owner.clone())
            .map_err(map_slot_error)?;
        let mut spec = match prepared.runtime.launch_spec() {
            Ok(spec) => spec,
            Err(_) => {
                let _ = self.runtime.abandon_start(&prepared.owner);
                return Err(CodexSessionError::Intent);
            }
        };
        spec.absolute_cleanup_deadline = Some(cleanup_deadline);
        let gate_prepared = match self.codex_gate.prepare_live() {
            Ok(gate) => gate,
            Err(error) => {
                let _ = self.runtime.abandon_start(&prepared.owner);
                return Err(map_gate_error(error));
            }
        };
        let process_lease = TreeCapacityPool::global()
            .try_acquire(spec.limits.descendants)
            .map_err(|_| {
                let _ = self.runtime.abandon_start(&prepared.owner);
                CodexSessionError::Capacity
            })?;
        let session_lease = match self.sessions.activate_session(command).await {
            Ok(lease) => lease,
            Err(mut failure) => {
                if let Some(recovery) = failure.recovery.take() {
                    match recovery.retry().await {
                        Ok(lease) => lease,
                        Err(mut retry) => {
                            let _ = self.runtime.mark_recovery(&prepared.owner);
                            if let Some(recovery) = retry.recovery.take() {
                                self.retain_codex_recovery(PendingCodexRecovery::Activation {
                                    recovery,
                                });
                            }
                            return Err(CodexSessionError::Recovery);
                        }
                    }
                } else {
                    let _ = self.runtime.abandon_start(&prepared.owner);
                    return Err(CodexSessionError::Session(failure.error));
                }
            }
        };
        let tree = match PlatformSupervisor::launch(&spec, process_lease) {
            Ok(tree) => tree,
            Err(failure) => {
                tracing::warn!(
                    launch_error = ?failure.error(),
                    runtime_epoch = prepared.owner.runtime_epoch.get(),
                    "Codex live session process launch failed"
                );
                return self
                    .abort_failed_launch(
                        &abort,
                        session_lease,
                        failure,
                        ActivationAbortCategory::LaunchFailure,
                    )
                    .await;
            }
        };
        let (mut stdin, stdout, control) = tree.into_parts();
        let active = match self.codex_gate.begin(&gate_prepared) {
            Ok(active) => active,
            Err(_) => {
                return self
                    .abort_pre_runner(
                        &abort,
                        session_lease,
                        &mut stdin,
                        &control,
                        cleanup_deadline,
                        ActivationAbortCategory::AssemblyFailure,
                        super::super::CodexLaunchSetupError::ActiveOperation,
                        None,
                    )
                    .await;
            }
        };
        if active.register_control(control.clone()).is_err() {
            return self
                .abort_pre_runner(
                    &abort,
                    session_lease,
                    &mut stdin,
                    &control,
                    cleanup_deadline,
                    ActivationAbortCategory::AssemblyFailure,
                    super::super::CodexLaunchSetupError::ControlRegistration,
                    Some(active),
                )
                .await;
        }
        let mailbox = match DriverMailbox::new() {
            Ok(mailbox) => Arc::new(mailbox),
            Err(_) => {
                return self
                    .abort_pre_runner(
                        &abort,
                        session_lease,
                        &mut stdin,
                        &control,
                        cleanup_deadline,
                        ActivationAbortCategory::AssemblyFailure,
                        super::super::CodexLaunchSetupError::Mailbox,
                        Some(active),
                    )
                    .await;
            }
        };
        prepared
            .runtime
            .attach_config(abort.owner.clone(), Arc::clone(&self.runtime.config));
        let pump = match ActivationPump::new_retained(
            prepared.runtime,
            CoordinatorActivationJournal::new(session_lease),
            resources,
            prepared.intent,
        ) {
            Ok(pump) => pump,
            Err(error) => {
                let (_, journal, _, _) = *error;
                let session_lease = journal
                    .into_pending_lease()
                    .map_err(|_| CodexSessionError::Recovery)?;
                return self
                    .abort_pre_runner(
                        &abort,
                        session_lease,
                        &mut stdin,
                        &control,
                        cleanup_deadline,
                        ActivationAbortCategory::AssemblyFailure,
                        super::super::CodexLaunchSetupError::PumpAssembly,
                        Some(active),
                    )
                    .await;
            }
        };
        let clock = Arc::new(SystemDriverClock) as Arc<dyn DriverClock>;
        let activation = match ActivationProcessDriver::new_retained(
            pump,
            stdin,
            control.clone(),
            abort.owner.clone(),
            Arc::clone(&clock),
        ) {
            Ok(activation) => activation,
            Err(failure) => {
                let (mut pump, mut stdin, control, _, _, _) = *failure;
                pump.force_teardown();
                let teardown = match pump.try_into_teardown_parts() {
                    Ok(teardown) => teardown,
                    Err(_) => return Err(CodexSessionError::Recovery),
                };
                let (_, journal, _) = teardown.into_parts();
                let session_lease = journal
                    .into_teardown_lease()
                    .map_err(|_| CodexSessionError::Recovery)?;
                return self
                    .abort_pre_runner(
                        &abort,
                        session_lease,
                        &mut stdin,
                        &control,
                        cleanup_deadline,
                        ActivationAbortCategory::AssemblyFailure,
                        super::super::CodexLaunchSetupError::DriverAssembly,
                        Some(active),
                    )
                    .await;
            }
        };
        let runner = LiveRunnerHandle::new(Arc::clone(&mailbox), control);
        #[cfg(test)]
        let finish_park_failure = {
            let mut failure = self
                .runtime_port_finish_park_failure
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *failure)
        };
        #[cfg(test)]
        let port = if finish_park_failure {
            let mut port = ProcessLivePort::new_managed(abort.owner.clone(), Arc::clone(&runner));
            port.set_finish_park_failure();
            port
        } else {
            ProcessLivePort::new_managed(abort.owner.clone(), Arc::clone(&runner))
        };
        #[cfg(not(test))]
        let port = ProcessLivePort::new_managed(abort.owner.clone(), Arc::clone(&runner));
        let host = match RuntimeHost::start_with_shutdown_recovery(
            LiveRuntimeEngine::new(port),
            Arc::clone(&runner),
        ) {
            Ok(host) => Arc::new(host),
            Err(_) => {
                return self
                    .abort_driver_before_runner(
                        &abort,
                        activation,
                        cleanup_deadline,
                        ActivationAbortCategory::AssemblyFailure,
                        super::super::CodexLaunchSetupError::HostAssembly,
                        None,
                        Some(active),
                    )
                    .await;
            }
        };
        if host.activate(abort.owner.clone()).is_err() {
            return self
                .abort_driver_before_runner(
                    &abort,
                    activation,
                    cleanup_deadline,
                    ActivationAbortCategory::AssemblyFailure,
                    super::super::CodexLaunchSetupError::HostActivation,
                    Some((Arc::clone(&host), false)),
                    Some(active),
                )
                .await;
        }
        if self
            .runtime
            .finish_start(&abort.owner, Arc::clone(&host))
            .is_err()
        {
            return self
                .abort_driver_before_runner(
                    &abort,
                    activation,
                    cleanup_deadline,
                    ActivationAbortCategory::AssemblyFailure,
                    super::super::CodexLaunchSetupError::RuntimeSlot,
                    Some((Arc::clone(&host), false)),
                    Some(active),
                )
                .await;
        }
        let context = FirstTurnWorkerContext {
            owner: abort.owner.clone(),
            coordinator: SendCoordinator::new(
                Arc::new(DatabaseAuthority(self.database.clone())),
                self.journal.clone(),
                Arc::clone(&self.publisher),
            ),
            journal: self.journal.clone(),
            publisher: Arc::clone(&self.publisher),
            clock,
            mailbox,
            session_recovery: Some(self.sessions.clone()),
            zero_sequence: AtomicU64::new(0),
        };
        let active = Arc::new(std::sync::Mutex::new(Some(active)));
        if let Err(failure) = runner.start_activation(
            activation,
            context,
            stdout,
            Arc::downgrade(&host),
            operation_deadline,
            cleanup_deadline,
        ) {
            let active = active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            return self
                .recover_runner_start(&abort, failure, host, runner, active)
                .await;
        }
        observe_runner(
            &self.runtime,
            &abort.owner,
            &runner,
            &self.codex_recovery,
            Arc::clone(&active),
            #[cfg(any(debug_assertions, feature = "diagnostics"))]
            Arc::clone(&self.codex_runtime_identity),
        );
        if self.runtime.publish_start(&abort.owner).is_err() {
            let _ = self.runtime.mark_recovery(&abort.owner);
            self.retain_codex_recovery(PendingCodexRecovery::Runner {
                owner: abort.owner.clone(),
                runner,
            });
            return Err(CodexSessionError::Recovery);
        }
        #[cfg(any(debug_assertions, feature = "diagnostics"))]
        {
            // A diagnostics failure never affects the admitted live runtime.
            // The observation is captured after publish from its retained root
            // handle, so a batch launcher correctly reports node.exe.
            let _ = self.codex_runtime_identity.publish(
                &abort.owner,
                super::super::runtime_identity::AdmittedEntrypointInput {
                    path: &spec.artifact.canonical_absolute_path,
                    sha256: spec.artifact.sha256,
                    bytes: spec.artifact.byte_len,
                    kind: spec.artifact.entrypoint_kind,
                    version: &prepared.harness_version,
                    binding_digest: &abort.binding_digest,
                },
                &runner.control(),
            );
        }
        #[cfg(windows)]
        if let Some(guard) = prepared.browser_mcp_guard.as_mut() {
            guard.commit();
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn fail_next_runtime_port_finish_park(&self) {
        *self
            .runtime_port_finish_park_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }
}
