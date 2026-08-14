use super::*;

impl AcpService {
    pub(crate) async fn activate_codex_session(
        &self,
        command: ActivateSessionCommand,
        project_root: CodexProjectRootAuthority,
    ) -> Result<(), CodexSessionError> {
        let root = ConfiguredResourceRoot {
            root_id: project_root.root_id().clone(),
            canonical_native_root: project_root.as_path().to_owned(),
        };
        let (command, mut prepared) = self.prepare_codex_activation(command, project_root)?;
        let abort = ActivationAbortIntent::from_prepared(&prepared);
        let resources = NativeResourceMapper::new([root]).map_err(|_| CodexSessionError::Intent)?;
        let (operation_deadline, cleanup_deadline) = deadlines()?;
        let mut spec = prepared
            .runtime
            .launch_spec()
            .map_err(|_| CodexSessionError::Intent)?;
        spec.absolute_cleanup_deadline = Some(cleanup_deadline);
        let gate_prepared = self.codex_gate.prepare_live().map_err(map_gate_error)?;
        self.runtime
            .reserve_start(prepared.owner.clone())
            .map_err(map_slot_error)?;
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
                        Some(active),
                    )
                    .await;
            }
        };
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
                        Some(active),
                    )
                    .await;
            }
        };
        let runner = LiveRunnerHandle::new(Arc::clone(&mailbox), control);
        let host = match RuntimeHost::start_with_shutdown_recovery(
            LiveRuntimeEngine::new(ProcessLivePort::new_managed(
                abort.owner.clone(),
                Arc::clone(&runner),
            )),
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
        );
        if self.runtime.publish_start(&abort.owner).is_err() {
            let _ = self.runtime.mark_recovery(&abort.owner);
            self.retain_codex_recovery(PendingCodexRecovery::Runner {
                owner: abort.owner.clone(),
                runner,
            });
            return Err(CodexSessionError::Recovery);
        }
        Ok(())
    }
}
