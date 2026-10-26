use super::*;

mod terminal;

impl ZeroProcessProof {
    pub(crate) fn from_supervisor(
        owner: RuntimeOwner,
        token: &ZeroProcessToken,
    ) -> Result<Self, SessionCoordinatorError> {
        if owner.runtime_epoch.get() != token.runtime_epoch() {
            return Err(SessionCoordinatorError::Intent);
        }
        Ok(Self { owner })
    }

    #[cfg(test)]
    pub(crate) fn confirmed(owner: RuntimeOwner) -> Self {
        Self { owner }
    }

    #[cfg(test)]
    pub(crate) fn confirmed_for_test(owner: RuntimeOwner) -> Self {
        Self { owner }
    }
}

impl SessionTreeLease {
    pub(crate) fn owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    #[cfg(test)]
    pub(crate) fn reserve_turn_for_test(
        &self,
    ) -> Result<crate::acp::scheduler::TurnReservation, SessionCoordinatorError> {
        self.tree
            .as_ref()
            .ok_or(SessionCoordinatorError::Converged)?
            .reserve_turn()
            .map_err(Into::into)
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    pub(crate) fn set_characterization_observation(
        &mut self,
        observation: crate::acp::characterization_observation::CharacterizationObservation,
    ) -> Result<(), SessionCoordinatorError> {
        if self.tree.is_none() || self.characterization_observation.is_some() {
            return Err(SessionCoordinatorError::Intent);
        }
        self.characterization_observation = Some(observation);
        Ok(())
    }

    #[cfg(feature = "acp-fixture")]
    pub(crate) fn characterization_observation(
        &self,
    ) -> Option<crate::acp::characterization_observation::CharacterizationObservation> {
        self.characterization_observation.clone()
    }

    /// Prepares a Turn through the exact tree lease retained by this live
    /// session. The store remains the only Sage Turn-ID authority and the
    /// scheduler reservation never escapes the session owner.
    pub(crate) async fn prepare_turn<A: ProjectIntentAuthority>(
        &self,
        coordinator: &SendCoordinator<A>,
        command: crate::acp::store::PrepareTurnCommand,
    ) -> Result<PrepareDispatch, Box<crate::acp::runtime::PrepareFailure>> {
        self.validate(
            &command.project_id,
            &command.chat_id,
            &command.session_id,
            command.runtime_epoch,
        )
        .map_err(|error| {
            Box::new(crate::acp::runtime::PrepareFailure {
                error: map_session_prepare_error(error),
                recovery: None,
            })
        })?;
        coordinator
            .prepare(
                self.tree.as_ref().ok_or_else(|| {
                    Box::new(crate::acp::runtime::PrepareFailure {
                        error: crate::acp::runtime::CoordinatorError::Scheduler(
                            SchedulerError::StaleToken,
                        ),
                        recovery: None,
                    })
                })?,
                command,
            )
            .await
    }

    pub(crate) async fn complete_activation(
        &mut self,
        command: CompleteActivationCommand,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        self.validate(
            &command.project_id,
            &command.chat_id,
            &command.session_id,
            command.runtime_epoch,
        )?;
        let mutation = self
            .coordinator
            .commit(
                JournalLane::Data,
                StoreMutation::CompleteActivation(command),
            )
            .await?;
        validate_mutation(&self.owner, &mutation)?;
        #[cfg(feature = "acp-fixture")]
        if mutation.disposition == crate::acp::store::MutationDisposition::Applied
            && let Some(observation) = &self.characterization_observation
        {
            observation.record(
                crate::acp::characterization_observation::CharacterizationObservationEvent::ReadyCommitted,
            );
        }
        self.coordinator
            .publish(&self.owner.project_id, &self.owner.chat_id, &mutation)?;
        Ok(mutation)
    }

    #[cfg(any(test, all(windows, feature = "acp-fixture")))]
    pub(crate) async fn fail_activation_after_zero_process(
        &mut self,
        command: FailActivationCommand,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        self.validate_proof(&proof)?;
        self.validate(
            &command.project_id,
            &command.chat_id,
            &command.session_id,
            command.runtime_epoch,
        )?;
        self.commit_terminal(StoreMutation::FailActivation(command), proof)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn abort_activation_after_zero_process(
        &mut self,
        command: crate::acp::store::AbortActivationCommand,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        self.abort_activation_after_zero_process_retained(command, proof)
            .await
            .map_err(|(error, _proof)| error)
    }

    /// Retains the one-shot zero-process proof and tree reservation until the
    /// durable abort and its projection publication are both known. This lets
    /// recovery retry the same command after a commit-unknown result without
    /// falsely advertising process capacity.
    pub(crate) async fn abort_activation_after_zero_process_retained(
        &mut self,
        command: crate::acp::store::AbortActivationCommand,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, (SessionCoordinatorError, ZeroProcessProof)> {
        if let Err(error) = self.validate_proof(&proof) {
            return Err((error, proof));
        }
        if let Err(error) = self.validate(
            &command.project_id,
            &command.chat_id,
            &command.session_id,
            command.runtime_epoch,
        ) {
            return Err((error, proof));
        }
        self.commit_terminal_retained(StoreMutation::AbortActivation(command), proof)
            .await
    }

    // The proof is intentionally returned by value so callers cannot reuse it
    // after a successful durable abort.
    #[allow(clippy::result_large_err)]
    pub(crate) fn abort_activation_after_zero_process_blocking_retained(
        &mut self,
        command: crate::acp::store::AbortActivationCommand,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, (SessionCoordinatorError, ZeroProcessProof)> {
        if let Err(error) = self.validate_proof(&proof) {
            return Err((error, proof));
        }
        if let Err(error) = self.validate(
            &command.project_id,
            &command.chat_id,
            &command.session_id,
            command.runtime_epoch,
        ) {
            return Err((error, proof));
        }
        let committed = match self.coordinator.commit_blocking(
            JournalLane::Control,
            StoreMutation::AbortActivation(command),
        ) {
            Ok(committed) => committed,
            Err(error) => return Err((error, proof)),
        };
        if let Err(error) = validate_mutation(&self.owner, &committed) {
            return Err((error, proof));
        }
        if let Err(error) =
            self.coordinator
                .publish(&self.owner.project_id, &self.owner.chat_id, &committed)
        {
            return Err((error, proof));
        }
        let Some(tree) = self.tree.take() else {
            return Err((SessionCoordinatorError::Converged, proof));
        };
        match tree.zero_process_converged_retained() {
            Ok(()) => Ok(committed),
            Err((tree, error)) => {
                self.tree = Some(tree);
                Err((error.into(), proof))
            }
        }
    }

    pub(super) fn converge(
        &mut self,
        proof: ZeroProcessProof,
    ) -> Result<(), SessionCoordinatorError> {
        self.validate_proof(&proof)?;
        self.tree
            .take()
            .ok_or(SessionCoordinatorError::Converged)?
            .zero_process_converged()?;
        Ok(())
    }

    pub(super) fn validate_proof(
        &self,
        proof: &ZeroProcessProof,
    ) -> Result<(), SessionCoordinatorError> {
        if proof.owner != self.owner {
            return Err(SessionCoordinatorError::Intent);
        }
        Ok(())
    }

    pub(super) fn validate(
        &self,
        project: &SageProjectId,
        chat: &SageChatId,
        session: &SageAgentSessionId,
        epoch: RuntimeEpoch,
    ) -> Result<(), SessionCoordinatorError> {
        if self.tree.is_none()
            || project != &self.owner.project_id
            || chat != &self.owner.chat_id
            || session != &self.owner.session_id
            || epoch != self.owner.runtime_epoch
        {
            return Err(SessionCoordinatorError::Intent);
        }
        Ok(())
    }
}

fn map_session_prepare_error(
    error: SessionCoordinatorError,
) -> crate::acp::runtime::CoordinatorError {
    match error {
        SessionCoordinatorError::Intent | SessionCoordinatorError::Converged => {
            crate::acp::runtime::CoordinatorError::Intent
        }
        SessionCoordinatorError::Scheduler(error) => {
            crate::acp::runtime::CoordinatorError::Scheduler(error)
        }
        SessionCoordinatorError::Journal(error) => {
            crate::acp::runtime::CoordinatorError::Journal(error)
        }
        SessionCoordinatorError::Projection(error) => {
            crate::acp::runtime::CoordinatorError::Projection(error)
        }
        SessionCoordinatorError::JournalProtocol => {
            crate::acp::runtime::CoordinatorError::JournalProtocol
        }
        SessionCoordinatorError::BootstrapRequired => {
            crate::acp::runtime::CoordinatorError::BootstrapRequired
        }
    }
}
