mod failure;
mod observation;
mod receipt;
mod support;
mod types;

use std::sync::{Arc, atomic::AtomicU64};

use crate::acp::{
    characterization_observation::CharacterizationObservationEvent,
    fixture::FixtureAdapter,
    process::{PlatformSupervisor, TreeCapacityPool, monotonic_tick},
    resource_mapper::NativeResourceMapper,
    runtime::{
        ActivationIntent, ActivationMode, ActivationProcessDriver, ActivationPump,
        CoordinatorActivationJournal, DriverClock, DriverMailbox, FirstTurnWorkerContext,
        HostCancelIntent, HostCloseIntent, HostOutcome, HostPromptIntent, LiveRunnerHandle,
        LiveRuntimeEngine, ProcessLivePort, RuntimeHost, SendCoordinator, SystemDriverClock,
    },
    store::{
        ActivateSessionCommand, CloseSessionCommand, PrepareTurnCommand, SessionMutation,
        StartSessionCommand,
    },
};
use sage_acp_protocol::{intent::ExternalId, scheduler::RuntimeOwner};

use super::AcpService;
use crate::project_registry::ProjectRootAuthority;
use observation::ObservedStdin;
use support::{
    DatabaseAuthority, command_owner, converge_failed_runtime, deadlines, launch_spec, load_roots,
    map_host_error, map_slot, native_text, observe_runner, validate_session_binding,
};
use types::BINDING_DIGEST;

pub(crate) use types::{
    BINDING_DIGEST_HEX, BINDING_ID, CharacterizationArtifact, CharacterizationBinding,
    CharacterizationCase, CharacterizationError,
};

impl AcpService {
    pub(crate) async fn start_characterization_session(
        &self,
        command: StartSessionCommand,
    ) -> Result<SessionMutation, CharacterizationError> {
        if self.characterization.is_none()
            || command.binding_id.as_str() != BINDING_ID
            || command.binding_digest != BINDING_DIGEST_HEX
            || command.model_id.is_some()
        {
            return Err(CharacterizationError::Intent);
        }
        self.lane_at(0)
            .sessions
            .start_session(command)
            .await
            .map_err(|_| CharacterizationError::Session)
    }

    pub(crate) async fn start_characterization(
        &self,
        command: ActivateSessionCommand,
        project_root: ProjectRootAuthority,
    ) -> Result<(), CharacterizationError> {
        let binding = self
            .characterization
            .as_ref()
            .ok_or(CharacterizationError::Artifact)?
            .clone();
        if command.artifact_digest != binding.artifact_digest() {
            return Err(CharacterizationError::Intent);
        }
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        validate_session_binding(&self.database, &owner)?;
        let roots = load_roots(&self.database, &owner)?;
        let primary = project_root.as_path().to_owned();
        let resources =
            NativeResourceMapper::new(roots).map_err(|_| CharacterizationError::Intent)?;
        let (operation_deadline, cleanup_deadline) = deadlines()?;
        let spec = launch_spec(
            &binding.artifact,
            binding.case,
            &owner,
            project_root,
            cleanup_deadline,
        )?;
        let external_session = ExternalId::new("externalSession", "fixture-persisted-session-1")
            .map_err(|_| CharacterizationError::Activation)?;
        let capabilities = FixtureAdapter::activation_capabilities();
        let native_root = native_text(&primary)?.to_owned();
        let adapter = FixtureAdapter::new(
            BINDING_DIGEST,
            owner.runtime_epoch,
            owner.session_id.clone(),
            external_session.clone(),
        )
        .map_err(|_| CharacterizationError::Activation)?;
        let mailbox = Arc::new(DriverMailbox::new().map_err(|_| CharacterizationError::Capacity)?);

        self.lane_at(0)
            .runtime
            .reserve_start(owner.clone())
            .map_err(map_slot)?;
        let process_lease = match TreeCapacityPool::global().try_acquire(spec.limits.descendants) {
            Ok(lease) => lease,
            Err(_) => {
                let _ = self.lane_at(0).runtime.abandon_start(&owner);
                return Err(CharacterizationError::Capacity);
            }
        };
        if binding.begin_receipt(&owner).is_err() {
            let _ = self.lane_at(0).runtime.abandon_start(&owner);
            return Err(CharacterizationError::Observation);
        }
        let mut session_lease = match self.lane_at(0).sessions.activate_session(command).await {
            Ok(lease) => lease,
            Err(failure) => {
                let recovery = failure.recovery.is_some();
                if recovery {
                    let _ = self.lane_at(0).runtime.mark_recovery(&owner);
                } else {
                    let _ = self.lane_at(0).runtime.abandon_start(&owner);
                }
                return Err(if recovery {
                    CharacterizationError::Recovery
                } else {
                    CharacterizationError::Session
                });
            }
        };
        let observation = binding.observation();
        if session_lease
            .set_characterization_observation(observation.clone())
            .is_err()
        {
            let _ = self.lane_at(0).runtime.mark_recovery(&owner);
            return Err(CharacterizationError::Observation);
        }
        let tree = match PlatformSupervisor::launch(&spec, process_lease) {
            Ok(tree) => tree,
            Err(failure) => {
                return self
                    .finish_launch_failure(owner, session_lease, binding.artifact_digest(), failure)
                    .await;
            }
        };
        let (stdin, stdout, control) = tree.into_parts();
        if control
            .set_characterization_observation(observation.clone())
            .is_err()
        {
            return Err(converge_failed_runtime(
                &self.lane_at(0).runtime,
                &owner,
                &control,
                cleanup_deadline,
            ));
        }
        let process = match control.process_identity_receipt() {
            Ok(receipt) => receipt,
            Err(_) => {
                return Err(converge_failed_runtime(
                    &self.lane_at(0).runtime,
                    &owner,
                    &control,
                    cleanup_deadline,
                ));
            }
        };
        let spawn_timing = control.spawn_timing_receipt();
        if binding
            .record_started(&owner, process, spawn_timing)
            .is_err()
        {
            return Err(converge_failed_runtime(
                &self.lane_at(0).runtime,
                &owner,
                &control,
                cleanup_deadline,
            ));
        }
        let stdin = ObservedStdin::new(stdin, observation, binding.case);
        let runner = LiveRunnerHandle::new(Arc::clone(&mailbox), control.clone());
        let host = match RuntimeHost::start_with_deadline_and_shutdown_recovery(
            LiveRuntimeEngine::new(ProcessLivePort::new_managed(
                owner.clone(),
                Arc::clone(&runner),
            )),
            operation_deadline,
            cleanup_deadline,
            Arc::clone(&runner),
        ) {
            Ok(host) => Arc::new(host),
            Err(_) => {
                return Err(converge_failed_runtime(
                    &self.lane_at(0).runtime,
                    &owner,
                    &control,
                    cleanup_deadline,
                ));
            }
        };
        let pump = match ActivationPump::new(
            adapter,
            CoordinatorActivationJournal::new(session_lease),
            resources,
            ActivationIntent {
                binding_digest: BINDING_DIGEST,
                artifact_digest: binding.artifact_digest(),
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                mode: ActivationMode::New {
                    native_root,
                    model: None,
                },
                curated_ceiling: capabilities.clone(),
                sage_policy: capabilities,
            },
        ) {
            Ok(pump) => pump,
            Err(_) => {
                let error = converge_failed_runtime(
                    &self.lane_at(0).runtime,
                    &owner,
                    &control,
                    cleanup_deadline,
                );
                let _ = host.shutdown(RuntimeHost::close_deadline());
                return Err(error);
            }
        };
        let clock = Arc::new(SystemDriverClock) as Arc<dyn DriverClock>;
        let activation = match ActivationProcessDriver::new(
            pump,
            stdin,
            control.clone(),
            owner.clone(),
            Arc::clone(&clock),
        ) {
            Ok(activation) => activation,
            Err(_) => {
                let error = converge_failed_runtime(
                    &self.lane_at(0).runtime,
                    &owner,
                    &control,
                    cleanup_deadline,
                );
                let _ = host.shutdown(RuntimeHost::close_deadline());
                return Err(error);
            }
        };
        let context = FirstTurnWorkerContext {
            owner: owner.clone(),
            coordinator: SendCoordinator::new(
                Arc::new(DatabaseAuthority(self.database.clone())),
                self.journal.clone(),
                Arc::clone(&self.publisher),
            ),
            journal: self.journal.clone(),
            publisher: Arc::clone(&self.publisher),
            clock,
            mailbox,
            session_recovery: Some(self.lane_at(0).sessions.clone()),
            zero_sequence: AtomicU64::new(0),
        };
        if runner
            .start_activation(
                activation,
                context,
                stdout,
                Arc::downgrade(&host),
                operation_deadline,
                cleanup_deadline,
            )
            .is_err()
            || host.activate(owner.clone()).is_err()
        {
            let _ = self.lane_at(0).runtime.mark_recovery(&owner);
            let _ = host.shutdown(RuntimeHost::close_deadline());
            return Err(CharacterizationError::Recovery);
        }
        if let Err(error) = self
            .lane_at(0)
            .runtime
            .finish_start(&owner, Arc::clone(&host))
        {
            let _ = self.lane_at(0).runtime.mark_recovery(&owner);
            let _ = host.shutdown(RuntimeHost::close_deadline());
            return Err(map_slot(error));
        }
        if observe_runner(&self.lane_at(0).runtime, &owner, &runner).is_err() {
            let _ = self.lane_at(0).runtime.mark_recovery(&owner);
            return Err(CharacterizationError::Recovery);
        }
        Ok(())
    }

    pub(crate) fn send_characterization(
        &self,
        command: PrepareTurnCommand,
    ) -> Result<HostOutcome, crate::acp::prompt_send::PromptSendError<CharacterizationError>> {
        let owner = command_owner(&command);
        self.lane_at(0)
            .runtime
            .for_owner(&owner)
            .map_err(map_slot)
            .map_err(crate::acp::prompt_send::PromptSendError::not_started)?
            .send(HostPromptIntent {
                command,
                access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .map_err(|failure| {
                failure.map(|error| map_host_error(&self.lane_at(0).runtime, &owner, error))
            })
    }

    pub(crate) fn cancel_characterization(
        &self,
        intent: HostCancelIntent,
    ) -> Result<HostOutcome, CharacterizationError> {
        let owner = intent.owner.clone();
        let host = self
            .lane_at(0)
            .runtime
            .for_owner(&owner)
            .map_err(map_slot)?;
        let tick = monotonic_tick().map_err(|_| CharacterizationError::Observation)?;
        self.characterization
            .as_ref()
            .ok_or(CharacterizationError::Observation)?
            .observation()
            .record(CharacterizationObservationEvent::CancelReceived(tick));
        host.cancel(intent)
            .map_err(|error| map_host_error(&self.lane_at(0).runtime, &owner, error))
    }

    #[cfg(test)]
    pub(crate) fn characterization_state(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<crate::acp::runtime::HostState, CharacterizationError> {
        self.lane_at(0)
            .runtime
            .for_owner(owner)
            .map_err(map_slot)?
            .state()
            .map_err(|error| map_host_error(&self.lane_at(0).runtime, owner, error))
    }

    pub(crate) fn close_characterization(
        &self,
        command: CloseSessionCommand,
    ) -> Result<HostOutcome, CharacterizationError> {
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        let host = self
            .lane_at(0)
            .runtime
            .for_owner(&owner)
            .map_err(map_slot)?;
        let observation = self
            .characterization
            .as_ref()
            .ok_or(CharacterizationError::Observation)?
            .observation();
        observation.record(CharacterizationObservationEvent::CloseReceived);
        let outcome = host.close(HostCloseIntent { command }).map_err(|_| {
            let _ = self.lane_at(0).runtime.mark_recovery(&owner);
            CharacterizationError::Recovery
        })?;
        host.shutdown(RuntimeHost::close_deadline()).map_err(|_| {
            let _ = self.lane_at(0).runtime.mark_recovery(&owner);
            CharacterizationError::Recovery
        })?;
        self.lane_at(0)
            .runtime
            .release(&owner, &host)
            .map_err(map_slot)?;
        let wal = self
            .database
            .converge_wal_for_observation()
            .map_err(|_| CharacterizationError::Observation)?;
        self.characterization
            .as_ref()
            .ok_or(CharacterizationError::Observation)?
            .complete_receipt(&owner, wal)?;
        Ok(outcome)
    }
}
