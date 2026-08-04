use super::*;

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
}

impl SessionTreeLease {
    pub(crate) fn owner(&self) -> &RuntimeOwner {
        &self.owner
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
        self.coordinator
            .publish(&self.owner.project_id, &self.owner.chat_id, &mutation)?;
        Ok(mutation)
    }

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

    pub(crate) async fn close_session(
        &mut self,
        command: CloseSessionCommand,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        self.validate(
            &command.project_id,
            &command.chat_id,
            &command.session_id,
            command.runtime_epoch,
        )?;
        let mutation = self
            .coordinator
            .commit(JournalLane::Control, StoreMutation::CloseSession(command))
            .await?;
        validate_mutation(&self.owner, &mutation)?;
        self.tree
            .as_ref()
            .ok_or(SessionCoordinatorError::Converged)?
            .begin_draining()?;
        self.coordinator
            .publish(&self.owner.project_id, &self.owner.chat_id, &mutation)?;
        Ok(mutation)
    }

    pub(crate) async fn finalize_close_after_zero_process(
        &mut self,
        command: FinalizeCloseCommand,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        self.finalize_close_after_zero_process_retained(command, proof)
            .await
            .map_err(|(error, _proof)| error)
    }

    pub(crate) async fn finalize_close_after_zero_process_retained(
        &mut self,
        command: FinalizeCloseCommand,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, (SessionCoordinatorError, ZeroProcessProof)> {
        if let Err(error) = self.validate_proof(&proof) {
            return Err((error, proof));
        }
        if let Err(error) = self.validate(
            &command.request.project_id,
            &command.request.chat_id,
            &command.request.session_id,
            command.request.runtime_epoch,
        ) {
            return Err((error, proof));
        }
        let committed = match self
            .coordinator
            .commit(JournalLane::Control, StoreMutation::FinalizeClose(command))
            .await
        {
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

    pub(crate) async fn park_after_zero_process(
        &mut self,
        command: ParkSessionCommand,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        self.validate_proof(&proof)?;
        self.validate(
            &command.project_id,
            &command.chat_id,
            &command.session_id,
            command.runtime_epoch,
        )?;
        self.commit_terminal(StoreMutation::ParkSession(command), proof)
            .await
    }

    async fn commit_terminal(
        &mut self,
        mutation: StoreMutation,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        let committed = self
            .coordinator
            .commit(JournalLane::Control, mutation)
            .await?;
        validate_mutation(&self.owner, &committed)?;
        let publication =
            self.coordinator
                .publish(&self.owner.project_id, &self.owner.chat_id, &committed);
        self.converge(proof)?;
        publication?;
        Ok(committed)
    }

    fn converge(&mut self, proof: ZeroProcessProof) -> Result<(), SessionCoordinatorError> {
        self.validate_proof(&proof)?;
        self.tree
            .take()
            .ok_or(SessionCoordinatorError::Converged)?
            .zero_process_converged()?;
        Ok(())
    }

    fn validate_proof(&self, proof: &ZeroProcessProof) -> Result<(), SessionCoordinatorError> {
        if proof.owner != self.owner {
            return Err(SessionCoordinatorError::Intent);
        }
        Ok(())
    }

    fn validate(
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
