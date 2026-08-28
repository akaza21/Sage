use super::*;

impl SessionTreeLease {
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
        if mutation.disposition == crate::acp::store::MutationDisposition::Applied {
            self.tree
                .as_ref()
                .ok_or(SessionCoordinatorError::Converged)?
                .begin_draining()?;
        }
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
        #[cfg(feature = "acp-fixture")]
        if committed.disposition == crate::acp::store::MutationDisposition::Applied
            && let Some(observation) = &self.characterization_observation
        {
            observation.record(
                crate::acp::characterization_observation::CharacterizationObservationEvent::CloseCommitted,
            );
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

    pub(super) async fn commit_terminal(
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

    pub(super) async fn commit_terminal_retained(
        &mut self,
        mutation: StoreMutation,
        proof: ZeroProcessProof,
    ) -> Result<SessionMutation, (SessionCoordinatorError, ZeroProcessProof)> {
        let committed = match self
            .coordinator
            .commit(JournalLane::Control, mutation)
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
}
